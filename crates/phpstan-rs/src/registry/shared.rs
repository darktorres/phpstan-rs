//! A value held behind a pointer, copied on first write.
//!
//! A clone of a `Shared` is a pointer copy; the first write through a handle
//! that shares its value copies the value, and later writes reach the copy.
//! A map of a class's members is held this way, so that a class cloned from
//! the as-built table, or written in one member, copies pointers rather than
//! every member.

use std::sync::Arc;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Shared<M>(Arc<M>);

impl<M> Shared<M> {
    pub fn new(value: M) -> Shared<M> {
        Shared(Arc::new(value))
    }

    /// Whether both handles hold the same value, not merely equal ones.
    pub fn ptr_eq(&self, other: &Shared<M>) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl<M> Clone for Shared<M> {
    fn clone(&self) -> Shared<M> {
        Shared(Arc::clone(&self.0))
    }
}

impl<M: Default> Default for Shared<M> {
    fn default() -> Shared<M> {
        Shared::new(M::default())
    }
}

impl<M> std::ops::Deref for Shared<M> {
    type Target = M;

    fn deref(&self) -> &M {
        &self.0
    }
}

impl<M: Clone> std::ops::DerefMut for Shared<M> {
    fn deref_mut(&mut self) -> &mut M {
        Arc::make_mut(&mut self.0)
    }
}

impl<M: PartialEq> PartialEq for Shared<M> {
    fn eq(&self, other: &Shared<M>) -> bool {
        self.ptr_eq(other) || *self.0 == *other.0
    }
}

impl<T, M: FromIterator<T>> FromIterator<T> for Shared<M> {
    fn from_iter<I: IntoIterator<Item = T>>(items: I) -> Shared<M> {
        Shared::new(M::from_iter(items))
    }
}

impl<M: Clone + IntoIterator> IntoIterator for Shared<M> {
    type Item = M::Item;
    type IntoIter = M::IntoIter;

    fn into_iter(self) -> M::IntoIter {
        Arc::unwrap_or_clone(self.0).into_iter()
    }
}

impl<'a, M> IntoIterator for &'a Shared<M>
where
    &'a M: IntoIterator,
{
    type Item = <&'a M as IntoIterator>::Item;
    type IntoIter = <&'a M as IntoIterator>::IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        (&*self.0).into_iter()
    }
}

impl<'a, M: Clone> IntoIterator for &'a mut Shared<M>
where
    &'a mut M: IntoIterator,
{
    type Item = <&'a mut M as IntoIterator>::Item;
    type IntoIter = <&'a mut M as IntoIterator>::IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        Arc::make_mut(&mut self.0).into_iter()
    }
}

impl<M: syntax::canonical::Canonical> syntax::canonical::Canonical for Shared<M> {
    fn canonical(&self, p: &mut syntax::canonical::Printer) {
        let Shared(value) = self;
        value.canonical(p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clone_shares_until_either_side_writes() {
        let mut a: Shared<Vec<u32>> = Shared::new(vec![1, 2]);
        let b = a.clone();
        assert!(a.ptr_eq(&b));
        a.push(3);
        assert!(!a.ptr_eq(&b));
        assert_eq!(*a, vec![1, 2, 3]);
        assert_eq!(*b, vec![1, 2]);
    }

    #[test]
    fn equal_content_is_equal_whether_or_not_it_is_shared() {
        let a: Shared<Vec<u32>> = Shared::new(vec![1]);
        let b: Shared<Vec<u32>> = Shared::new(vec![1]);
        assert!(!a.ptr_eq(&b));
        assert_eq!(a, b);
        assert_ne!(a, Shared::new(vec![2]));
    }
}
