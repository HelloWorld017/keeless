use keeless_kdbx::{CompositeKey, EntryFieldId, OtpType, TokenCalculator, parse_otpauth_uri};
use keeless_schema::{GetEntryTotpArgs, GetEntryTotpResult, OperationSuccess};
use zeroize::{Zeroize, Zeroizing};

use crate::model::parse_node_id;
use crate::{CoreError, KeelessCore, Result};

pub(crate) async fn run(
    core: &mut KeelessCore,
    entry_id: keeless_schema::DatabaseNodeId,
    field_id: String,
    password: Option<&[u8]>,
) -> Result<GetEntryTotpResult> {
    if core.handle.is_none() {
        return Err(CoreError::DatabaseLocked);
    }
    let entry_id = parse_node_id(entry_id)?;
    let field_id = field_id
        .parse::<EntryFieldId>()
        .map_err(|_| CoreError::InvalidEntryField)?;
    let handle = core.handle.as_ref().ok_or(CoreError::DatabaseLocked)?;
    let entry = handle
        .database()
        .get_entry(&entry_id)
        .ok_or(CoreError::EntryNotFound)?;
    let field = entry.field(field_id).ok_or(CoreError::InvalidEntryField)?;
    if !field.value().is_protected() {
        return Err(CoreError::InvalidEntryField);
    }

    let key = if let Some(password) = password {
        let key = CompositeKey::new().with_password(password)?;
        core.handle
            .as_ref()
            .ok_or(CoreError::DatabaseLocked)?
            .verify_credentials(&key)?;
        key
    } else if let Some(credential) = &core.credential {
        credential.restore_key()?
    } else {
        let password = core
            .request_password(crate::PasswordInputMode::Reveal)
            .await?;
        let key = CompositeKey::new().with_password(&password)?;
        core.handle
            .as_ref()
            .ok_or(CoreError::DatabaseLocked)?
            .verify_credentials(&key)?;
        key
    };

    let now_ms = core.clock.now_millis();
    if now_ms < 0 {
        return Err(CoreError::InvalidTotp);
    }
    let result = core
        .handle
        .as_ref()
        .ok_or(CoreError::DatabaseLocked)?
        .database()
        .with_entry_field_id(&key, &entry_id, field_id, |value| {
            let uri = Zeroizing::new(value.to_owned());
            let mut params = parse_otpauth_uri(&uri).ok_or(CoreError::InvalidTotp)?;
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
        })
        .map_err(CoreError::from)??;
    core.touch_activity();
    Ok(result)
}

pub(super) async fn execute(
    core: &mut KeelessCore,
    mut args: GetEntryTotpArgs,
) -> Result<OperationSuccess> {
    let password = args
        .password
        .take()
        .map(|password| Zeroizing::new(password.into_bytes()));
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
