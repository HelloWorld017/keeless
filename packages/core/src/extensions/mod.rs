use std::any::{Any, TypeId};
use std::collections::HashMap;

use keeless_kdbx::{CompositeKey, Database};

use crate::Result;

pub(crate) mod passkey;
pub(crate) mod password_session;
pub(crate) mod sync;

pub(crate) trait CoreExtension: Any + Send + Sync {
    fn unlock(&mut self, database: &Database, key: &CompositeKey) -> Result<()>;
    fn lock(&mut self);
    fn tick(&mut self, monotonic_millis: u64);
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

pub(crate) struct Extensions {
    values: HashMap<TypeId, Box<dyn CoreExtension>>,
}

impl Extensions {
    pub(crate) fn new() -> Result<Self> {
        let mut values: HashMap<TypeId, Box<dyn CoreExtension>> = HashMap::new();
        values.insert(
            TypeId::of::<passkey::PasskeyExtension>(),
            Box::new(passkey::PasskeyExtension::new()?),
        );
        values.insert(
            TypeId::of::<password_session::PasswordSessionExtension>(),
            Box::new(password_session::PasswordSessionExtension::new()),
        );
        Ok(Self { values })
    }

    pub(crate) fn unlock(&mut self, database: &Database, key: &CompositeKey) -> Result<()> {
        for extension in self.values.values_mut() {
            extension.unlock(database, key)?;
        }
        Ok(())
    }

    pub(crate) fn lock(&mut self) {
        for extension in self.values.values_mut() {
            extension.lock();
        }
    }

    pub(crate) fn tick(&mut self, monotonic_millis: u64) {
        for extension in self.values.values_mut() {
            extension.tick(monotonic_millis);
        }
    }

    pub(crate) fn get_mut<T: CoreExtension>(&mut self) -> &mut T {
        self.values
            .get_mut(&TypeId::of::<T>())
            .and_then(|extension| extension.as_any_mut().downcast_mut())
            .expect("registered core extension has its declared type")
    }

    pub(crate) fn get<T: CoreExtension>(&self) -> &T {
        self.values
            .get(&TypeId::of::<T>())
            .and_then(|extension| extension.as_any().downcast_ref())
            .expect("registered core extension has its declared type")
    }
}
