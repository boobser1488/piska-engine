//! Загрузка динамических библиотек: `libloading` там, где есть `dlopen`/`LoadLibrary`.
//!
//! В браузере динамических библиотек нет: тот же API отвечает понятной ошибкой,
//! чтобы `Plugin::load` и `CppPlugins::load_hot` компилировались везде, а игра
//! получала сообщение, а не падение.

#[cfg(any(unix, windows))]
pub(crate) use libloading::{Error, Library};

#[cfg(not(any(unix, windows)))]
pub(crate) use unsupported::{Error, Library};

#[cfg(not(any(unix, windows)))]
mod unsupported {
    use std::ffi::OsStr;
    use std::marker::PhantomData;

    /// Ошибка загрузки на платформе без динамических библиотек.
    #[derive(Debug)]
    pub struct Error;

    impl std::fmt::Display for Error {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(
                "this platform cannot load dynamic libraries (web build); \
                 C++ plugins must be linked statically",
            )
        }
    }

    impl std::error::Error for Error {}

    /// Библиотека, которую нельзя открыть: значение этого типа не создаётся никогда.
    #[derive(Debug)]
    pub struct Library(std::convert::Infallible);

    /// Символ библиотеки (повторяет `libloading::Symbol`); не создаётся, как и `Library`.
    #[allow(dead_code)]
    pub struct Symbol<'lib, T> {
        value: T,
        _library: PhantomData<&'lib ()>,
    }

    impl<T> std::ops::Deref for Symbol<'_, T> {
        type Target = T;
        fn deref(&self) -> &T {
            &self.value
        }
    }

    impl Library {
        /// # Safety
        /// Та же сигнатура, что у `libloading::Library::new`; здесь всегда ошибка.
        pub unsafe fn new<P: AsRef<OsStr>>(_filename: P) -> Result<Self, Error> {
            Err(Error)
        }

        /// # Safety
        /// Та же сигнатура, что у `libloading::Library::get`; вызвать нельзя — `Library` не существует.
        pub unsafe fn get<T>(&self, _symbol: &[u8]) -> Result<Symbol<'_, T>, Error> {
            match self.0 {}
        }
    }
}
