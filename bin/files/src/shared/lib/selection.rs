//! Copy-on-write ordered selection set with `O(1)` cloning, range slicing, and membership lookup.

use std::{
    borrow::Borrow,
    collections::{HashMap, HashSet, hash_map::Entry},
    fmt,
    hash::Hash,
    ops::Index,
    sync::Arc,
};

/// Shared ordered backing slice and `O(1)` position index for contiguous range selections.
///
/// Items are deduplicated on construction in first-seen order so every index in `items`
/// maps 1-to-1 with its position in `positions`.
pub(crate) struct SelectionBacking<T> {
    items: Box<[T]>,
    positions: HashMap<T, usize>,
}

impl<T> SelectionBacking<T> {
    pub(crate) fn from_items<I>(iter: I) -> Arc<Self>
    where
        I: IntoIterator<Item = T>,
        T: Clone + Eq + Hash,
    {
        let iter = iter.into_iter();
        let (lower, _) = iter.size_hint();
        let mut items = Vec::with_capacity(lower);
        let mut positions = HashMap::with_capacity(lower);
        for item in iter {
            let next_index = items.len();
            if let Entry::Vacant(slot) = positions.entry(item.clone()) {
                slot.insert(next_index);
                items.push(item);
            }
        }
        Arc::new(Self {
            items: items.into_boxed_slice(),
            positions,
        })
    }

    pub(crate) fn len(&self) -> usize {
        self.items.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub(crate) fn items(&self) -> &[T] {
        &self.items
    }

    pub(crate) fn position<Q>(&self, value: &Q) -> Option<usize>
    where
        T: Borrow<Q> + Eq + Hash,
        Q: Hash + Eq + ?Sized,
    {
        self.positions.get(value).copied()
    }
}

impl<T: fmt::Debug> fmt::Debug for SelectionBacking<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.items.iter()).finish()
    }
}

#[derive(Clone)]
struct Inner<T> {
    order: Vec<T>,
    set: HashSet<T>,
}

enum Repr<T> {
    Empty,
    /// Non-empty inclusive range `start..=end` into `backing.items` (`start <= end < backing.len()`).
    Range {
        backing: Arc<SelectionBacking<T>>,
        start: usize,
        end: usize,
    },
    Set(Arc<Inner<T>>),
}

impl<T> Clone for Repr<T> {
    fn clone(&self) -> Self {
        match self {
            Self::Empty => Self::Empty,
            Self::Range {
                backing,
                start,
                end,
            } => Self::Range {
                backing: Arc::clone(backing),
                start: *start,
                end: *end,
            },
            Self::Set(inner) => Self::Set(Arc::clone(inner)),
        }
    }
}

/// Ordered selection set backed by a shared copy-on-write allocation or a zero-copy range slice.
///
/// Empty selections remain [`Repr::Empty`] so [`Default`] performs zero heap allocation,
/// contiguous range selections share [`SelectionBacking`] in `O(1)`, and arbitrary selections
/// keep `order` (unique items in first-seen order) synchronized with `set` (`O(1)` membership).
pub(crate) struct Selection<T> {
    repr: Repr<T>,
}

impl<T> Selection<T> {
    pub(crate) const fn new() -> Self {
        Self { repr: Repr::Empty }
    }

    pub(crate) fn len(&self) -> usize {
        match &self.repr {
            Repr::Empty => 0,
            Repr::Range { start, end, .. } => end - start + 1,
            Repr::Set(inner) => inner.order.len(),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        matches!(self.repr, Repr::Empty)
    }

    pub(crate) fn as_slice(&self) -> &[T] {
        match &self.repr {
            Repr::Empty => &[],
            Repr::Range {
                backing,
                start,
                end,
            } => &backing.items()[*start..=*end],
            Repr::Set(inner) => inner.order.as_slice(),
        }
    }

    pub(crate) fn first(&self) -> Option<&T> {
        self.as_slice().first()
    }

    pub(crate) fn last(&self) -> Option<&T> {
        self.as_slice().last()
    }

    pub(crate) fn iter(&self) -> std::slice::Iter<'_, T> {
        self.as_slice().iter()
    }

    pub(crate) fn contains<Q>(&self, value: &Q) -> bool
    where
        T: Borrow<Q> + Eq + Hash,
        Q: Hash + Eq + ?Sized,
    {
        match &self.repr {
            Repr::Empty => false,
            Repr::Range {
                backing,
                start,
                end,
            } => backing
                .position(value)
                .is_some_and(|index| index >= *start && index <= *end),
            Repr::Set(inner) => inner.set.contains(value),
        }
    }

    pub(crate) fn to_vec(&self) -> Vec<T>
    where
        T: Clone,
    {
        self.as_slice().to_vec()
    }

    pub(crate) fn ptr_eq(&self, other: &Self) -> bool {
        match (&self.repr, &other.repr) {
            (Repr::Empty, Repr::Empty) => true,
            (
                Repr::Range {
                    backing: left_backing,
                    start: left_start,
                    end: left_end,
                },
                Repr::Range {
                    backing: right_backing,
                    start: right_start,
                    end: right_end,
                },
            ) => {
                Arc::ptr_eq(left_backing, right_backing)
                    && left_start == right_start
                    && left_end == right_end
            }
            (Repr::Set(left), Repr::Set(right)) => Arc::ptr_eq(left, right),
            _ => false,
        }
    }

    pub(crate) fn clear(&mut self) {
        self.repr = Repr::Empty;
    }

    /// Select the inclusive span `start..=end` from `backing` in `O(1)` without cloning or hashing.
    pub(crate) fn set_range(
        &mut self,
        backing: &Arc<SelectionBacking<T>>,
        start: usize,
        end: usize,
    ) {
        if backing.is_empty() || start > end || end >= backing.len() {
            self.repr = Repr::Empty;
            return;
        }
        if let Repr::Range {
            backing: current,
            start: current_start,
            end: current_end,
        } = &self.repr
            && Arc::ptr_eq(current, backing)
            && *current_start == start
            && *current_end == end
        {
            return;
        }
        self.repr = Repr::Range {
            backing: Arc::clone(backing),
            start,
            end,
        };
    }

    pub(crate) fn set_single(&mut self, item: T)
    where
        T: Clone + Eq + Hash,
    {
        if self.len() == 1 && self.as_slice()[0] == item {
            return;
        }
        let mut set = HashSet::with_capacity(1);
        set.insert(item.clone());
        self.repr = Repr::Set(Arc::new(Inner {
            order: vec![item],
            set,
        }));
    }

    pub(crate) fn replace<I>(&mut self, iter: I)
    where
        I: IntoIterator<Item = T>,
        T: Clone + Eq + Hash,
    {
        let iter = iter.into_iter();
        let (lower, _) = iter.size_hint();
        let mut order = Vec::with_capacity(lower);
        let mut set = HashSet::with_capacity(lower);
        for item in iter {
            if set.insert(item.clone()) {
                order.push(item);
            }
        }
        if order.is_empty() {
            self.repr = Repr::Empty;
            return;
        }
        if self.as_slice() == order.as_slice() {
            return;
        }
        self.repr = Repr::Set(Arc::new(Inner { order, set }));
    }

    pub(crate) fn insert(&mut self, item: T) -> bool
    where
        T: Clone + Eq + Hash,
    {
        if self.contains(&item) {
            return false;
        }
        match &mut self.repr {
            Repr::Empty => {
                let mut set = HashSet::with_capacity(1);
                set.insert(item.clone());
                self.repr = Repr::Set(Arc::new(Inner {
                    order: vec![item],
                    set,
                }));
            }
            Repr::Range {
                backing,
                start,
                end,
            } => {
                let slice = &backing.items[*start..=*end];
                let capacity = slice.len() + 1;
                let mut order = Vec::with_capacity(capacity);
                let mut set = HashSet::with_capacity(capacity);
                for existing in slice {
                    order.push(existing.clone());
                    set.insert(existing.clone());
                }
                set.insert(item.clone());
                order.push(item);
                self.repr = Repr::Set(Arc::new(Inner { order, set }));
            }
            Repr::Set(arc) => {
                let inner = Arc::make_mut(arc);
                inner.set.insert(item.clone());
                inner.order.push(item);
            }
        }
        true
    }

    pub(crate) fn remove<Q>(&mut self, value: &Q) -> bool
    where
        T: Borrow<Q> + Clone + Eq + Hash,
        Q: Hash + Eq + ?Sized,
    {
        match &mut self.repr {
            Repr::Empty => false,
            Repr::Range {
                backing,
                start,
                end,
            } => {
                let Some(pos) = backing.position(value) else {
                    return false;
                };
                if pos < *start || pos > *end {
                    return false;
                }
                if *start == *end {
                    self.repr = Repr::Empty;
                    return true;
                }
                let capacity = *end - *start;
                let mut order = Vec::with_capacity(capacity);
                let mut set = HashSet::with_capacity(capacity);
                for item in backing.items[*start..pos]
                    .iter()
                    .chain(&backing.items[(pos + 1)..=*end])
                {
                    order.push(item.clone());
                    set.insert(item.clone());
                }
                self.repr = Repr::Set(Arc::new(Inner { order, set }));
                true
            }
            Repr::Set(arc) => {
                if !arc.set.contains(value) {
                    return false;
                }
                if arc.order.len() == 1 {
                    self.repr = Repr::Empty;
                    return true;
                }
                let inner = Arc::make_mut(arc);
                inner.set.remove(value);
                inner.order.retain(|item| item.borrow() != value);
                if inner.order.is_empty() {
                    self.repr = Repr::Empty;
                }
                true
            }
        }
    }

    pub(crate) fn toggle(&mut self, item: T) -> bool
    where
        T: Clone + Eq + Hash,
    {
        if self.remove(&item) {
            false
        } else {
            self.insert(item);
            true
        }
    }

    pub(crate) fn retain<F>(&mut self, mut keep: F)
    where
        F: FnMut(&T) -> bool,
        T: Clone + Eq + Hash,
    {
        match &self.repr {
            Repr::Empty => {}
            Repr::Range {
                backing,
                start,
                end,
            } => {
                let slice = &backing.items[*start..=*end];
                let Some(first_removed) = slice.iter().position(|item| !keep(item)) else {
                    return;
                };
                let mut order = Vec::with_capacity(slice.len() - 1);
                let mut set = HashSet::with_capacity(slice.len() - 1);
                for item in &slice[..first_removed] {
                    order.push(item.clone());
                    set.insert(item.clone());
                }
                for item in &slice[(first_removed + 1)..] {
                    if keep(item) {
                        order.push(item.clone());
                        set.insert(item.clone());
                    }
                }
                self.repr = if order.is_empty() {
                    Repr::Empty
                } else {
                    Repr::Set(Arc::new(Inner { order, set }))
                };
            }
            Repr::Set(arc) => {
                // Avoid cloning or mutating the shared allocation when every element stays selected.
                let Some(first_removed) = arc.order.iter().position(|item| !keep(item)) else {
                    return;
                };
                let Repr::Set(arc) = &mut self.repr else {
                    return;
                };
                if let Some(inner) = Arc::get_mut(arc) {
                    let removed = inner.order.split_off(first_removed);
                    let mut removed_iter = removed.into_iter();
                    if let Some(first) = removed_iter.next() {
                        inner.set.remove(&first);
                    }
                    for item in removed_iter {
                        if keep(&item) {
                            inner.order.push(item);
                        } else {
                            inner.set.remove(&item);
                        }
                    }
                    if inner.order.is_empty() {
                        self.repr = Repr::Empty;
                    }
                } else {
                    let mut order = Vec::with_capacity(arc.order.len() - 1);
                    let mut set = HashSet::with_capacity(arc.order.len() - 1);
                    for item in &arc.order[..first_removed] {
                        order.push(item.clone());
                        set.insert(item.clone());
                    }
                    for item in &arc.order[(first_removed + 1)..] {
                        if keep(item) {
                            order.push(item.clone());
                            set.insert(item.clone());
                        }
                    }
                    self.repr = if order.is_empty() {
                        Repr::Empty
                    } else {
                        Repr::Set(Arc::new(Inner { order, set }))
                    };
                }
            }
        }
    }
}

impl<T> Clone for Selection<T> {
    fn clone(&self) -> Self {
        Self {
            repr: self.repr.clone(),
        }
    }
}

impl<T> Default for Selection<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: fmt::Debug> fmt::Debug for Selection<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.as_slice()).finish()
    }
}

impl<T: PartialEq> PartialEq for Selection<T> {
    fn eq(&self, other: &Self) -> bool {
        if self.ptr_eq(other) {
            return true;
        }
        if let (
            Repr::Range {
                backing: left_backing,
                start: left_start,
                end: left_end,
            },
            Repr::Range {
                backing: right_backing,
                start: right_start,
                end: right_end,
            },
        ) = (&self.repr, &other.repr)
            && Arc::ptr_eq(left_backing, right_backing)
        {
            return left_start == right_start && left_end == right_end;
        }
        let left = self.as_slice();
        let right = other.as_slice();
        left.len() == right.len() && left == right
    }
}

impl<T: Eq> Eq for Selection<T> {}

impl<T: PartialEq> PartialEq<Vec<T>> for Selection<T> {
    fn eq(&self, other: &Vec<T>) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl<T: PartialEq> PartialEq<Selection<T>> for Vec<T> {
    fn eq(&self, other: &Selection<T>) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl<T: PartialEq> PartialEq<[T]> for Selection<T> {
    fn eq(&self, other: &[T]) -> bool {
        self.as_slice() == other
    }
}

impl<T: PartialEq> PartialEq<&[T]> for Selection<T> {
    fn eq(&self, other: &&[T]) -> bool {
        self.as_slice() == *other
    }
}

impl<T: PartialEq, const N: usize> PartialEq<[T; N]> for Selection<T> {
    fn eq(&self, other: &[T; N]) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl<T: PartialEq, const N: usize> PartialEq<&[T; N]> for Selection<T> {
    fn eq(&self, other: &&[T; N]) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl<T> AsRef<[T]> for Selection<T> {
    fn as_ref(&self) -> &[T] {
        self.as_slice()
    }
}

impl<T> Index<usize> for Selection<T> {
    type Output = T;

    fn index(&self, index: usize) -> &Self::Output {
        &self.as_slice()[index]
    }
}

impl<'a, T> IntoIterator for &'a Selection<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<T: Clone> IntoIterator for Selection<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        match self.repr {
            Repr::Empty => Vec::new().into_iter(),
            Repr::Range {
                backing,
                start,
                end,
            } => backing.items[start..=end].to_vec().into_iter(),
            Repr::Set(arc) => Arc::try_unwrap(arc)
                .map(|inner| inner.order)
                .unwrap_or_else(|arc| arc.order.clone())
                .into_iter(),
        }
    }
}

impl<T: Clone + Eq + Hash> FromIterator<T> for Selection<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        let mut selection = Self::new();
        selection.replace(iter);
        selection
    }
}

impl<T: Clone + Eq + Hash> From<Vec<T>> for Selection<T> {
    fn from(items: Vec<T>) -> Self {
        items.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{Repr, Selection, SelectionBacking};
    use std::{
        collections::HashSet,
        path::{Path, PathBuf},
    };

    #[test]
    fn preserves_insertion_order_and_deduplicates() {
        let mut selection = Selection::<&str>::default();
        assert!(selection.is_empty());
        assert_eq!(selection.len(), 0);
        assert!(matches!(selection.repr, Repr::Empty));

        selection.replace(["beta", "alpha", "beta", "gamma", "alpha"]);
        assert_eq!(selection, vec!["beta", "alpha", "gamma"]);
        assert_eq!(selection.as_slice(), &["beta", "alpha", "gamma"]);
        assert_eq!(selection.first(), Some(&"beta"));
        assert_eq!(selection.last(), Some(&"gamma"));
        assert!(!selection.insert("alpha"));
        assert!(selection.insert("delta"));
        assert_eq!(selection, vec!["beta", "alpha", "gamma", "delta"]);

        assert!(!selection.toggle("alpha"));
        assert_eq!(selection, vec!["beta", "gamma", "delta"]);
        assert!(selection.toggle("alpha"));
        assert_eq!(selection, vec!["beta", "gamma", "delta", "alpha"]);
    }

    #[test]
    fn supports_borrowed_path_lookup_and_collapses_when_empty() {
        let mut selection = Selection::new();
        let alpha = PathBuf::from("/workspace/alpha.txt");
        let beta = PathBuf::from("/workspace/beta.txt");
        selection.replace([alpha.clone(), beta.clone()]);

        assert!(selection.contains(Path::new("/workspace/alpha.txt")));
        assert!(selection.contains(&beta));
        assert!(!selection.contains(Path::new("/workspace/missing.txt")));

        assert!(selection.remove(Path::new("/workspace/alpha.txt")));
        assert!(!selection.contains(Path::new("/workspace/alpha.txt")));
        assert_eq!(selection, vec![beta]);

        assert!(selection.remove(Path::new("/workspace/beta.txt")));
        assert!(selection.is_empty());
        assert!(matches!(selection.repr, Repr::Empty));
    }

    #[test]
    fn cow_mutations_preserve_shared_allocation_on_no_ops() {
        let mut selection = Selection::new();
        selection.set_single(String::from("first"));
        let snapshot = selection.clone();
        assert!(selection.ptr_eq(&snapshot));

        selection.set_single(String::from("first"));
        assert!(selection.ptr_eq(&snapshot));

        assert!(!selection.insert(String::from("first")));
        assert!(!selection.remove("absent"));
        selection.retain(|item| !item.is_empty());
        assert!(selection.ptr_eq(&snapshot));

        assert!(selection.insert(String::from("second")));
        assert_eq!(
            selection,
            vec![String::from("first"), String::from("second")]
        );
        assert_eq!(snapshot, vec![String::from("first")]);
        assert!(!selection.ptr_eq(&snapshot));

        selection.retain(|_| false);
        assert!(selection.is_empty());
        assert!(matches!(selection.repr, Repr::Empty));
    }

    #[test]
    fn large_selection_lookup_and_retain_scale_without_quadratic_scans() {
        let mut selection: Selection<PathBuf> = (0..10_000)
            .map(|index| PathBuf::from(format!("/srv/files/entry-{index}.dat")))
            .collect();
        assert_eq!(selection.len(), 10_000);
        assert!(selection.contains(Path::new("/srv/files/entry-0.dat")));
        assert!(selection.contains(Path::new("/srv/files/entry-4096.dat")));
        assert!(selection.contains(Path::new("/srv/files/entry-9999.dat")));
        assert!(!selection.contains(Path::new("/srv/files/entry-10000.dat")));

        let shared = selection.clone();
        selection.retain(|path| !path.as_os_str().is_empty());
        assert!(selection.ptr_eq(&shared));

        let visible: HashSet<PathBuf> = [
            PathBuf::from("/srv/files/entry-5.dat"),
            PathBuf::from("/srv/files/entry-500.dat"),
            PathBuf::from("/srv/files/entry-9995.dat"),
        ]
        .into_iter()
        .collect();
        selection.retain(|path| visible.contains(path));
        assert_eq!(
            selection,
            vec![
                PathBuf::from("/srv/files/entry-5.dat"),
                PathBuf::from("/srv/files/entry-500.dat"),
                PathBuf::from("/srv/files/entry-9995.dat"),
            ]
        );
        assert_eq!(shared.len(), 10_000);
    }

    #[test]
    fn backing_deduplicates_and_supports_constant_time_range_selection() {
        let backing = SelectionBacking::from_items([
            PathBuf::from("/srv/a"),
            PathBuf::from("/srv/b"),
            PathBuf::from("/srv/a"),
            PathBuf::from("/srv/c"),
        ]);
        assert_eq!(backing.len(), 3);
        assert!(!backing.is_empty());
        assert_eq!(backing.position(Path::new("/srv/a")), Some(0));
        assert_eq!(backing.position(Path::new("/srv/b")), Some(1));
        assert_eq!(backing.position(Path::new("/srv/c")), Some(2));
        assert_eq!(backing.position(Path::new("/srv/missing")), None);

        let large_backing = SelectionBacking::from_items(
            (0..10_000).map(|index| PathBuf::from(format!("/srv/files/entry-{index}.dat"))),
        );
        let mut selection = Selection::new();
        let mut previous = Selection::new();
        for end in 0..1_000 {
            selection.set_range(&large_backing, 0, end);
            assert_eq!(selection.len(), end + 1);
            assert!(selection.contains(Path::new("/srv/files/entry-0.dat")));
            assert!(selection.contains(Path::new(&format!("/srv/files/entry-{end}.dat"))));
            assert!(!selection.contains(Path::new(&format!("/srv/files/entry-{}.dat", end + 1))));
            assert_ne!(selection, previous);
            previous = selection.clone();
            assert!(selection.ptr_eq(&previous));
            assert_eq!(selection, previous);
        }
        assert_eq!(selection.as_slice(), &large_backing.items()[0..=999]);
    }

    #[test]
    fn range_transitions_cleanly_across_retain_toggle_insert_and_remove() {
        let backing = SelectionBacking::from_items(["a", "b", "c", "d", "e"]);
        let mut selection = Selection::new();
        selection.set_range(&backing, 1, 3);
        assert_eq!(selection.as_slice(), &["b", "c", "d"]);
        assert!(!selection.contains(&"a"));
        assert!(selection.contains(&"b"));
        assert!(selection.contains(&"d"));
        assert!(!selection.contains(&"e"));

        // No-op retain keeps the zero-copy Range representation intact.
        let snapshot = selection.clone();
        selection.retain(|item| *item != "z");
        assert!(selection.ptr_eq(&snapshot));
        assert!(matches!(selection.repr, Repr::Range { .. }));

        // Toggle off an interior element materializes a Set without mutating the snapshot.
        assert!(!selection.toggle("c"));
        assert_eq!(selection.as_slice(), &["b", "d"]);
        assert_eq!(snapshot.as_slice(), &["b", "c", "d"]);

        // Insert appends in order.
        assert!(selection.insert("a"));
        assert_eq!(selection.as_slice(), &["b", "d", "a"]);

        // Out-of-bounds or inverted range collapses to Empty.
        selection.set_range(&backing, 3, 1);
        assert!(selection.is_empty());
        selection.set_range(&backing, 0, 10);
        assert!(selection.is_empty());
    }
}
