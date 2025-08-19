use futures::{Stream, StreamExt};
use std::{future::Future, iter};

pub fn transform_raw_stream<Raw, Target, E>(
    stream: impl Stream<Item = Result<Raw, E>> + Send + 'static,
) -> impl Stream<Item = Result<Target, E>> + Send + 'static
where
    Target: TryFrom<Raw, Error = E> + Send + 'static,
    Raw: Send + 'static,
    E: Send + 'static,
{
    stream.map(|res| res.and_then(Target::try_from))
}

pub fn transform_raw_stream_with<Raw, Target, E, F>(
    stream: impl Stream<Item = Result<Raw, E>> + Send + 'static,
    mut convert_fn: F,
) -> impl Stream<Item = Result<Target, E>> + Send + 'static
where
    F: FnMut(Raw) -> Result<Target, E> + Send + 'static,
    Raw: Send + 'static,
    Target: Send + 'static,
    E: Send + 'static,
{
    stream.map(move |res| res.and_then(&mut convert_fn))
}

pub fn transform_raw_vec_stream<Raw, Target, E>(
    stream: impl Stream<Item = Result<Raw, E>> + Send + 'static,
) -> impl Stream<Item = Result<Target, E>> + Send + 'static
where
    Vec<Target>: TryFrom<Raw, Error = E>,
    Raw: Send + 'static,
    Target: Send + 'static,
    E: Send + 'static,
{
    stream.flat_map(|res| {
        let iterator = res.and_then(Vec::<Target>::try_from).map_or_else(
            |err| itertools::Either::Right(iter::once(Err(err))),
            |vec| itertools::Either::Left(vec.into_iter().map(Ok)),
        );
        // The iterator itself must be Send to be used across .await points in a multi-threaded context.
        futures::stream::iter(iterator)
    })
}

pub fn transform_raw_vec_stream_with<Raw, Target, E, F>(
    stream: impl Stream<Item = Result<Raw, E>> + Send + 'static,
    mut convert_fn: F,
) -> impl Stream<Item = Result<Target, E>> + Send + 'static
where
    F: FnMut(Raw) -> Result<Vec<Target>, E> + Send + 'static,
    Raw: Send + 'static,
    Target: Send + 'static,
    E: Send + 'static,
{
    stream.flat_map(move |res| {
        let iterator = res.and_then(&mut convert_fn).map_or_else(
            |err| itertools::Either::Right(iter::once(Err(err))),
            |vec| itertools::Either::Left(vec.into_iter().map(Ok)),
        );
        futures::stream::iter(iterator)
    })
}
