//! definition of Bind and Monad traits based monadic macro

pub trait Monad {
    type Container<A>;
    type Item;
    fn bind<U, F>(self, f: F) -> Self::Container<U>
    where
        F: FnOnce(Self::Item) -> Self::Container<U>,
        Self: Sized;

    fn pure(x: Self::Item) -> Self;
}

impl<T, E> Monad for Result<T, E> {
    type Container<A> = Result<A, E>;
    type Item = T;
    fn pure(x: T) -> Self {
        Ok(x)
    }
    fn bind<U, F>(self, f: F) -> Self::Container<U>
    where
        F: FnOnce(Self::Item) -> Self::Container<U>,
        Self: Sized,
    {
        self.and_then(f)
    }
}

/// macro for iterables (IntoIterator) as monads enabling monad comprehensions over iterables
///
/// You can use:
/// * `monadic_expression`       to end with a monad expression
/// * `v <- monadic_expression`  to use the monad result
/// * `&v <- &container`  to use a reference item result from a by reference container
/// * `_ <- monadic_expression`  to ignore the monad result
/// * `let z = expression`       to combine monad results
///
macro_rules! _mdo {
  (_ <- $monad:expr ; $($rest:tt)* ) => [($monad).bind( |_| { mdo!($($rest)*)} )];
  (&$v:ident <- $monad:expr ; $($rest:tt)* ) => [($monad).bind( |&$v| { mdo!($($rest)*)} )];
  ($v:ident <- $monad:expr ; $($rest:tt)* ) => [($monad).bind( |$v| { mdo!($($rest)*)} )];
  ($monad:expr                            ) => [$monad];
}

pub(crate) use _mdo as mdo;
