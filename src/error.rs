/// Errors in netwatcher or in one of the underlying platform integratinos.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    CreateSocket(String),
    Bind(String),
    CreatePipe(String),
    Getifaddrs(String),
    GetInterfaceName(String),
    FormatMacAddress,
    UnexpectedWindowsResult(u32),
    AddressNotAssociated,
    InvalidParameter,
    NotEnoughMemory,
    InvalidHandle,
    NoAndroidContext,
    Jni(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl std::error::Error for Error {}

#[cfg(target_os = "android")]
impl From<jni::errors::Error> for Error {
    fn from(err: jni::errors::Error) -> Self {
        Error::Jni(err.to_string())
    }
}
