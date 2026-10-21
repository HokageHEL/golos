//! Portable clipboard and single-use request decisions.
//!
//! This module deliberately contains no AppKit calls. The macOS adapter feeds
//! it a complete snapshot of raw bytes; these types then enforce the parent
//! `dictation` safety contract in one place that can be tested on every target.

use std::collections::{HashMap, VecDeque};

use crate::dictation::{ClipboardLease, InsertRefusal, RestoreDecision};

/// A complete rich snapshot can be large. These limits are intentionally
/// finite: an upload-sized clipboard is refused before the general pasteboard
/// is changed rather than being silently truncated.
pub const DEFAULT_MAX_CLIPBOARD_BYTES: usize = 32 * 1024 * 1024;
pub const DEFAULT_MAX_CLIPBOARD_ITEMS: usize = 64;
pub const DEFAULT_MAX_TYPES_PER_ITEM: usize = 64;
/// Bounded duplicate-request memory. Request IDs are expected to be UUIDs; the
/// oldest entries are evicted only after this many insertions.
pub const DEFAULT_REQUEST_HISTORY: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipboardLimits {
    pub max_bytes: usize,
    pub max_items: usize,
    pub max_types_per_item: usize,
}

impl Default for ClipboardLimits {
    fn default() -> Self {
        Self {
            max_bytes: DEFAULT_MAX_CLIPBOARD_BYTES,
            max_items: DEFAULT_MAX_CLIPBOARD_ITEMS,
            max_types_per_item: DEFAULT_MAX_TYPES_PER_ITEM,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotError {
    TooManyItems,
    TooManyTypes,
    TooLarge,
    /// A promised or lazily-provided type returned no data. Writing the
    /// transcript and restoring only the types that happened to resolve would
    /// destroy the rest of the user's clipboard.
    PromisedDataUnavailable,
    /// The pasteboard access behavior is explicitly deny.
    Denied,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotType {
    pub kind: String,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SnapshotItem {
    pub types: Vec<SnapshotType>,
}

/// Raw, owned bytes for every item and type. The macOS adapter converts this
/// to `NSData`/`NSPasteboardItem` values only on the main thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardSnapshot {
    pub items: Vec<SnapshotItem>,
    pub original_change_count: i64,
    /// False represents a lossy snapshot. Such a snapshot may be used only
    /// with the explicit restore-disabled preference.
    pub complete: bool,
}

impl ClipboardSnapshot {
    pub fn total_bytes(&self) -> usize {
        self.items
            .iter()
            .flat_map(|item| item.types.iter())
            .map(|value| value.data.len())
            .sum()
    }

    pub fn lease(&self, written_count: i64, restore_previous: bool) -> ClipboardLease {
        ClipboardLease {
            original_count: self.original_change_count,
            written_count,
            complete_snapshot: self.complete,
            restore_previous,
        }
    }
}

/// Enforces the snapshot limits while the adapter copies each type. The
/// adapter checks `data.len()` before materialising a copy, so an oversized
/// type is rejected without allocating its bytes.
#[derive(Debug)]
pub struct SnapshotBuilder {
    limits: ClipboardLimits,
    snapshot: ClipboardSnapshot,
}

impl SnapshotBuilder {
    pub fn new(original_change_count: i64, limits: ClipboardLimits) -> Self {
        Self {
            limits,
            snapshot: ClipboardSnapshot {
                items: Vec::new(),
                original_change_count,
                complete: true,
            },
        }
    }

    pub fn item_count(&self) -> usize {
        self.snapshot.items.len()
    }

    pub fn add_item(&mut self, types: Vec<SnapshotType>) -> Result<(), SnapshotError> {
        if self.snapshot.items.len() >= self.limits.max_items {
            return Err(SnapshotError::TooManyItems);
        }
        if types.len() > self.limits.max_types_per_item {
            return Err(SnapshotError::TooManyTypes);
        }
        let added = types
            .iter()
            .try_fold(0usize, |total, value| total.checked_add(value.data.len()))
            .ok_or(SnapshotError::TooLarge)?;
        let total = self
            .snapshot
            .total_bytes()
            .checked_add(added)
            .ok_or(SnapshotError::TooLarge)?;
        if total > self.limits.max_bytes {
            return Err(SnapshotError::TooLarge);
        }
        self.snapshot.items.push(SnapshotItem { types });
        Ok(())
    }

    pub fn finish(self) -> ClipboardSnapshot {
        self.snapshot
    }
}

/// One insertion's clipboard ownership checks. `authorize_write` is called
/// after capture/revalidation and before the pasteboard is cleared;
/// `authorize_paste` is called again immediately before the first event post.
#[derive(Debug)]
pub struct ClipboardTransaction {
    restore_previous: bool,
    snapshot: Option<ClipboardSnapshot>,
    original_count: i64,
    written_count: Option<i64>,
    dispatch_consumed: bool,
}

impl ClipboardTransaction {
    pub fn prepare(
        original_count: i64,
        restore_previous: bool,
        snapshot: Option<ClipboardSnapshot>,
    ) -> Self {
        Self {
            restore_previous,
            snapshot,
            original_count,
            written_count: None,
            dispatch_consumed: false,
        }
    }

    pub fn authorize_write(&self, current_count: i64) -> Result<(), InsertRefusal> {
        if self.restore_previous {
            let Some(snapshot) = &self.snapshot else {
                return Err(InsertRefusal::ClipboardUnavailable);
            };
            if !snapshot.complete {
                return Err(InsertRefusal::ClipboardUnavailable);
            }
        }
        if current_count != self.original_count {
            Err(InsertRefusal::ClipboardChanged)
        } else {
            Ok(())
        }
    }

    pub fn note_written(&mut self, written_count: i64) {
        self.written_count = Some(written_count);
    }

    pub fn authorize_paste(&self, current_count: i64) -> Result<(), InsertRefusal> {
        match self.written_count {
            Some(written) if current_count == written => Ok(()),
            _ => Err(InsertRefusal::ClipboardChanged),
        }
    }

    /// Must be called before the first `CGEventPostToPid` call. A second call
    /// can never dispatch again, including when the first response was lost.
    pub fn consume_dispatch(&mut self) -> Result<(), InsertRefusal> {
        if self.dispatch_consumed {
            Err(InsertRefusal::AlreadyDispatched)
        } else {
            self.dispatch_consumed = true;
            Ok(())
        }
    }

    pub fn restore_snapshot(&self) -> Option<&ClipboardSnapshot> {
        if self.restore_previous {
            self.snapshot.as_ref().filter(|snapshot| snapshot.complete)
        } else {
            None
        }
    }

    pub fn restore_decision(&self, current_count: i64) -> RestoreDecision {
        let written = self.written_count.unwrap_or(self.original_count);
        match &self.snapshot {
            Some(snapshot) => snapshot
                .lease(written, self.restore_previous)
                .restore(current_count),
            None => ClipboardLease {
                original_count: self.original_count,
                written_count: written,
                complete_snapshot: false,
                restore_previous: false,
            }
            .restore(current_count),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedgerReservation {
    Reserved,
    Duplicate,
}

/// Bounded duplicate-request guard. Reserving happens before any main-thread
/// work, so a repeated request ID is refused even if the first response was
/// lost after dispatch.
#[derive(Debug)]
pub struct RequestLedger {
    max_entries: usize,
    order: VecDeque<String>,
    known: HashMap<String, bool>,
}

impl RequestLedger {
    pub fn new(max_entries: usize) -> Self {
        Self {
            max_entries: max_entries.max(1),
            order: VecDeque::new(),
            known: HashMap::new(),
        }
    }

    pub fn reserve(&mut self, request_id: &str) -> LedgerReservation {
        if self.known.contains_key(request_id) {
            return LedgerReservation::Duplicate;
        }
        while self.order.len() >= self.max_entries {
            if let Some(oldest) = self.order.pop_front() {
                self.known.remove(&oldest);
            }
        }
        self.order.push_back(request_id.to_string());
        self.known.insert(request_id.to_string(), false);
        LedgerReservation::Reserved
    }

    pub fn mark_dispatched(&mut self, request_id: &str) {
        if let Some(value) = self.known.get_mut(request_id) {
            *value = true;
        }
    }

    #[cfg(test)]
    pub fn is_dispatched(&self, request_id: &str) -> bool {
        self.known.get(request_id).copied().unwrap_or(false)
    }

    pub fn len(&self) -> usize {
        self.known.len()
    }

    pub fn is_empty(&self) -> bool {
        self.known.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rich_snapshot() -> ClipboardSnapshot {
        ClipboardSnapshot {
            original_change_count: 10,
            complete: true,
            items: vec![
                SnapshotItem {
                    types: vec![
                        SnapshotType {
                            kind: "public.utf8-plain-text".into(),
                            data: b"hello".to_vec(),
                        },
                        SnapshotType {
                            kind: "public.tiff".into(),
                            data: vec![0, 1, 2, 255],
                        },
                    ],
                },
                SnapshotItem {
                    types: vec![SnapshotType {
                        kind: "com.example.binary".into(),
                        data: vec![9, 8, 7],
                    }],
                },
            ],
        }
    }

    #[test]
    fn complete_rich_snapshot_keeps_binary_data_and_restores_only_while_ours() {
        let snapshot = rich_snapshot();
        assert_eq!(snapshot.items.len(), 2);
        assert_eq!(snapshot.items[0].types[1].data, vec![0, 1, 2, 255]);
        assert_eq!(snapshot.total_bytes(), 12);
        let transaction = ClipboardTransaction::prepare(10, true, Some(snapshot));
        assert_eq!(transaction.authorize_write(10), Ok(()));
        assert_eq!(
            transaction.authorize_write(11),
            Err(InsertRefusal::ClipboardChanged)
        );
        let mut transaction = transaction;
        transaction.note_written(11);
        assert_eq!(transaction.authorize_paste(11), Ok(()));
        assert_eq!(
            transaction.authorize_paste(12),
            Err(InsertRefusal::ClipboardChanged)
        );
        assert_eq!(transaction.restore_decision(11), RestoreDecision::Restore);
        assert_eq!(
            transaction.restore_decision(12),
            RestoreDecision::KeepNewerClipboard
        );
        assert_eq!(
            transaction.restore_decision(13),
            RestoreDecision::KeepNewerClipboard
        );
    }

    #[test]
    fn dispatch_is_consumed_once_even_after_a_lost_response() {
        let mut transaction = ClipboardTransaction::prepare(1, false, None);
        assert_eq!(transaction.consume_dispatch(), Ok(()));
        assert_eq!(
            transaction.consume_dispatch(),
            Err(InsertRefusal::AlreadyDispatched)
        );
    }

    #[test]
    fn lossy_or_missing_snapshot_refuses_restore_but_explicit_no_restore_is_allowed() {
        let mut lossy = rich_snapshot();
        lossy.complete = false;
        let restore = ClipboardTransaction::prepare(10, true, Some(lossy.clone()));
        assert_eq!(
            restore.authorize_write(10),
            Err(InsertRefusal::ClipboardUnavailable)
        );
        assert!(restore.restore_snapshot().is_none());
        let keep = ClipboardTransaction::prepare(10, false, Some(lossy));
        assert_eq!(keep.authorize_write(10), Ok(()));
        assert!(keep.restore_snapshot().is_none());
        let missing = ClipboardTransaction::prepare(10, true, None);
        assert_eq!(
            missing.authorize_write(10),
            Err(InsertRefusal::ClipboardUnavailable)
        );
    }

    #[test]
    fn snapshot_builder_refuses_oversized_items_types_and_counts() {
        let limits = ClipboardLimits {
            max_bytes: 8,
            max_items: 1,
            max_types_per_item: 2,
        };
        let mut builder = SnapshotBuilder::new(1, limits);
        assert_eq!(
            builder.add_item(vec![
                SnapshotType {
                    kind: "a".into(),
                    data: vec![0; 4]
                },
                SnapshotType {
                    kind: "b".into(),
                    data: vec![0; 5]
                }
            ]),
            Err(SnapshotError::TooLarge)
        );
        assert_eq!(
            builder.add_item(vec![
                SnapshotType {
                    kind: "a".into(),
                    data: vec![]
                };
                3
            ]),
            Err(SnapshotError::TooManyTypes)
        );
        builder
            .add_item(vec![SnapshotType {
                kind: "a".into(),
                data: vec![1, 2, 3],
            }])
            .unwrap();
        assert_eq!(
            builder.add_item(vec![SnapshotType {
                kind: "b".into(),
                data: vec![]
            }]),
            Err(SnapshotError::TooManyItems)
        );
        let snapshot = builder.finish();
        assert!(snapshot.complete);
        assert_eq!(snapshot.total_bytes(), 3);
    }

    #[test]
    fn request_ledger_refuses_duplicates_and_stays_bounded() {
        let mut ledger = RequestLedger::new(3);
        assert_eq!(ledger.reserve("a"), LedgerReservation::Reserved);
        assert_eq!(ledger.reserve("a"), LedgerReservation::Duplicate);
        ledger.mark_dispatched("a");
        assert!(ledger.is_dispatched("a"));
        assert_eq!(ledger.reserve("b"), LedgerReservation::Reserved);
        assert_eq!(ledger.reserve("c"), LedgerReservation::Reserved);
        assert_eq!(ledger.reserve("d"), LedgerReservation::Reserved);
        assert_eq!(ledger.len(), 3);
        // Oldest history may be evicted only after the bounded window.
        assert_eq!(ledger.reserve("a"), LedgerReservation::Reserved);
    }
}
