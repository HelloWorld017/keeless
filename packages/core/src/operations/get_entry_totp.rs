use keeless_kdbx::{
    CompositeCredentials, EntryFieldId, EntryFieldSelector, KEEPASS_TIMEOTP_FIELD_NAMES,
    OtpParameters, OtpType, TokenCalculator, is_keepass_timeotp_secret_field,
    parse_keepass_timeotp_fields, parse_otpauth_uri,
};
use keeless_schema::{GetEntryTotpArgs, GetEntryTotpResult, OperationSuccess};
use zeroize::{Zeroize, Zeroizing};

use crate::model::parse_node_id;
use crate::{
    CoreError, KeelessCore, Result, extensions::password_session::PasswordSessionExtension,
};

pub(crate) async fn run(
    core: &mut KeelessCore,
    entry_id: keeless_schema::DatabaseNodeId,
    field_id: Option<String>,
    password: Option<&[u8]>,
) -> Result<GetEntryTotpResult> {
    if core.handle.is_none() {
        return Err(CoreError::DatabaseLocked);
    }
    let entry_id = parse_node_id(entry_id)?;
    let field_id = field_id
        .map(|field_id| {
            field_id
                .parse::<EntryFieldId>()
                .map_err(|_| CoreError::InvalidEntryField)
        })
        .transpose()?;
    let handle = core.handle.as_ref().ok_or(CoreError::DatabaseLocked)?;
    let entry = handle
        .database()
        .get_entry(&entry_id)
        .ok_or(CoreError::EntryNotFound)?;
    let timeotp_fields = if let Some(field_id) = field_id {
        let field = entry.field(field_id).ok_or(CoreError::InvalidEntryField)?;
        if !field.value().is_protected() {
            return Err(CoreError::InvalidEntryField);
        }
        None
    } else {
        let mut fields = Vec::new();
        for name in KEEPASS_TIMEOTP_FIELD_NAMES {
            let count = entry
                .custom_fields()
                .filter(|(_, field)| field.name() == name)
                .count();
            if count > 1 {
                return Err(CoreError::InvalidTotp);
            }
            if count == 1 {
                fields.push(EntryFieldSelector::Custom(name.into()));
            }
        }
        if !fields.iter().any(|field| {
            matches!(field, EntryFieldSelector::Custom(name) if is_keepass_timeotp_secret_field(name))
        }) {
            return Err(CoreError::InvalidTotp);
        }
        Some(fields)
    };

    let key = if let Some(password) = password {
        let credentials = CompositeCredentials::new().with_password(password)?;
        core.handle
            .as_ref()
            .ok_or(CoreError::DatabaseLocked)?
            .derive_key(&credentials)?
    } else if let Some(credential) = &core.credential {
        credential.restore_key()?
    } else {
        let password = core
            .request_password(crate::PasswordInputMode::Reveal)
            .await?;
        let credentials = CompositeCredentials::new().with_password(&password)?;
        core.handle
            .as_ref()
            .ok_or(CoreError::DatabaseLocked)?
            .derive_key(&credentials)?
    };

    let now_ms = core.clock.now_millis();
    if now_ms < 0 {
        return Err(CoreError::InvalidTotp);
    }
    let database = core
        .handle
        .as_ref()
        .ok_or(CoreError::DatabaseLocked)?
        .database();
    let params = if let Some(field_id) = field_id {
        database
            .with_entry_field_id(&key, &entry_id, field_id, |value| {
                let uri = Zeroizing::new(value.to_owned());
                parse_otpauth_uri(&uri).ok_or(CoreError::InvalidTotp)
            })
            .map_err(CoreError::from)??
    } else {
        let mut values = Vec::new();
        for field in timeotp_fields.expect("TimeOtp fields are set without an OTP field ID") {
            let EntryFieldSelector::Custom(name) = &field else {
                unreachable!();
            };
            let value = database
                .with_entry_field(&key, &entry_id, &field, |value| {
                    Zeroizing::new(value.to_owned())
                })
                .map_err(CoreError::from)?;
            values.push((name.clone(), value));
        }
        parse_keepass_timeotp_fields(
            values
                .iter()
                .map(|(name, value)| (name.as_str(), value.as_str())),
        )
        .ok_or(CoreError::InvalidTotp)?
    };
    let result = calculate_totp(params, now_ms)?;
    core.touch_activity();
    Ok(result)
}

fn calculate_totp(mut params: OtpParameters, now_ms: i64) -> Result<GetEntryTotpResult> {
    let result = if params.otp_type != OtpType::Totp {
        Err(CoreError::InvalidTotp)
    } else {
        let period_ms = i64::from(params.period) * 1_000;
        let expires_at_ms = now_ms
            .div_euclid(period_ms)
            .saturating_add(1)
            .saturating_mul(period_ms);
        let code = TokenCalculator::format_code(
            TokenCalculator::totp_at(&params, now_ms as u64 / 1_000),
            params.digits,
        );
        Ok(GetEntryTotpResult {
            code,
            digits: params.digits,
            period: params.period,
            expires_at_ms,
        })
    };
    params.secret.zeroize();
    params.issuer.zeroize();
    params.account.zeroize();
    result
}

pub(super) async fn execute(
    core: &mut KeelessCore,
    mut args: GetEntryTotpArgs,
) -> Result<OperationSuccess> {
    let password = {
        let now_millis = core.clock.monotonic_millis();
        core.extensions
            .get_mut::<PasswordSessionExtension>()
            .resolve_argument(
                args.password.take(),
                args.password_session.take(),
                now_millis,
            )?
    };
    Ok(OperationSuccess::GetEntryTotp(
        run(
            core,
            args.entry_id,
            args.field_id,
            password.as_ref().map(|password| password.as_slice()),
        )
        .await?,
    ))
}
