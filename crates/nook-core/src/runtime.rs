//! Tiny helpers around the shared Tokio runtime.

pub fn block_on<F: std::future::Future>(future: F) -> F::Output {
    crate::runtime().block_on(future)
}

pub fn spawn<F>(future: F)
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    crate::runtime().spawn(future);
}
