use std::fmt;
use std::ops::Deref;
use std::sync::{Arc, Weak};

type Destructor<T> = Box<dyn Fn(&mut T) + Send + Sync>;

struct Inner<T> {
    metadata: T,
    dropper: Destructor<T>,
}

impl<T: fmt::Debug> fmt::Debug for Inner<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Inner")
            .field("metadata", &self.metadata)
            .finish()
    }
}

impl<T> Drop for Inner<T> {
    fn drop(&mut self) {
        (self.dropper)(&mut self.metadata);
    }
}

#[derive(Debug, Clone)]
pub struct Resource<T> {
    inner: Arc<Inner<T>>,
}

#[derive(Debug, Clone)]
pub struct WeakResource<T> {
    inner: Weak<Inner<T>>,
}

impl<T> Resource<T> {
    pub fn new<F>(metadata: T, destructor: F) -> Self
    where
        F: Fn(&mut T) + Send + Sync + 'static,
    {
        Self {
            inner: Arc::new(Inner {
                metadata,
                dropper: Box::new(destructor),
            }),
        }
    }

    pub fn downgrade(&self) -> WeakResource<T> {
        WeakResource {
            inner: Arc::downgrade(&self.inner),
        }
    }

    pub fn strong_count(&self) -> usize {
        Arc::strong_count(&self.inner)
    }

    pub fn weak_count(&self) -> usize {
        Arc::weak_count(&self.inner)
    }
}

impl<T> WeakResource<T> {
    pub fn upgrade(&self) -> Option<Resource<T>> {
        self.inner.upgrade().map(|arc| Resource { inner: arc })
    }

    pub fn strong_count(&self) -> usize {
        self.inner.strong_count()
    }
}

impl<T> Deref for Resource<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.inner.metadata
    }
}
