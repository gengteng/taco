use std::error::Error;

pub type Exception = Box<dyn Error + Sync + Send + 'static>;
pub type WeoResult<T> = Result<T, Exception>;
