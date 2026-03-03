// Stub - will be implemented by memory-dev agent
use flowforge_core::{Error, Result};

pub struct MemoryDb;

impl MemoryDb {
    pub fn open(_path: &std::path::Path) -> Result<Self> {
        Err(Error::Memory("stub".into()))
    }
}
