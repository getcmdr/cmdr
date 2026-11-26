use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::ReadOnce;

fn counted<'a>(reads: &'a AtomicUsize, answer: Option<&'static str>) -> impl FnOnce() -> Option<&'static str> + 'a {
    move || {
        reads.fetch_add(1, Ordering::SeqCst);
        answer
    }
}

/// A miss is remembered: a second listing (⌘R) of a server with nothing stored asks no
/// second time.
#[test]
fn a_miss_is_read_once() {
    let memo = ReadOnce::default();
    let reads = AtomicUsize::new(0);

    assert_eq!(memo.read("smb://testnas", || None, counted(&reads, None)), None);
    assert_eq!(memo.read("smb://testnas", || None, counted(&reads, None)), None);

    assert_eq!(reads.load(Ordering::SeqCst), 1, "the store was read once");
    assert!(memo.is_spent("smb://testnas"));
}

/// A hit answers from memory afterwards (the keychain's cache holds it), and the store
/// is never asked again.
#[test]
fn a_hit_is_read_once_and_then_answered_from_memory() {
    let memo = ReadOnce::default();
    let reads = AtomicUsize::new(0);
    let cache = std::sync::Mutex::new(None::<&'static str>);

    let first = memo.read(
        "smb://testnas",
        || *cache.lock().expect("test lock"),
        || {
            reads.fetch_add(1, Ordering::SeqCst);
            *cache.lock().expect("test lock") = Some("testuser");
            Some("testuser")
        },
    );
    let second = memo.read(
        "smb://testnas",
        || *cache.lock().expect("test lock"),
        counted(&reads, Some("other")),
    );

    assert_eq!(first, Some("testuser"));
    assert_eq!(second, Some("testuser"));
    assert_eq!(reads.load(Ordering::SeqCst), 1);
}

/// Something already in memory (a sheet's answer) needs no read at all.
#[test]
fn a_known_answer_spends_no_read() {
    let memo = ReadOnce::default();
    let reads = AtomicUsize::new(0);

    let answer = memo.read("smb://testnas", || Some("typed"), counted(&reads, Some("stored")));

    assert_eq!(answer, Some("typed"));
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert!(!memo.is_spent("smb://testnas"), "the read is still there for later");
}

/// Each server gets its own read.
#[test]
fn servers_are_read_apart() {
    let memo = ReadOnce::default();
    let reads = AtomicUsize::new(0);

    memo.read("smb://testnas", || None, counted(&reads, None));
    memo.read("smb://testnas:11482", || None, counted(&reads, None));

    assert_eq!(reads.load(Ordering::SeqCst), 2);
}

/// ❗ One server's pending read (a system prompt, up to a minute) never holds up another
/// server's first read: the turn is per server.
#[test]
fn a_pending_read_blocks_only_its_own_server() {
    use std::sync::mpsc;
    use std::time::Duration;

    let memo = Arc::new(ReadOnce::default());
    let (entered_tx, entered_rx) = mpsc::channel::<()>();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let slow = {
        let memo = Arc::clone(&memo);
        std::thread::spawn(move || {
            memo.read(
                "smb://testnas",
                || None::<&'static str>,
                move || {
                    entered_tx.send(()).expect("test channel");
                    // The prompt waits on a person until the test releases it.
                    release_rx.recv().expect("test channel");
                    None
                },
            )
        })
    };
    entered_rx.recv().expect("the slow read started");

    let (done_tx, done_rx) = mpsc::channel();
    let other = {
        let memo = Arc::clone(&memo);
        std::thread::spawn(move || {
            let answer = memo.read("smb://attic", || None, || Some("testuser"));
            done_tx.send(answer).expect("test channel");
        })
    };
    let other_answer = done_rx.recv_timeout(Duration::from_secs(5));
    release_tx.send(()).expect("test channel");
    slow.join().expect("slow reader");
    other.join().expect("other reader");

    assert_eq!(
        other_answer,
        Ok(Some("testuser")),
        "the other server read while the first prompt was pending"
    );
}

/// Two callers racing for one server (a listing and a mount) read once between them.
#[test]
fn racing_callers_read_once() {
    let memo = Arc::new(ReadOnce::default());
    let reads = Arc::new(AtomicUsize::new(0));

    let handles: Vec<_> = (0..8)
        .map(|_| {
            let memo = Arc::clone(&memo);
            let reads = Arc::clone(&reads);
            std::thread::spawn(move || {
                memo.read(
                    "smb://testnas",
                    || None::<()>,
                    || {
                        reads.fetch_add(1, Ordering::SeqCst);
                        None
                    },
                )
            })
        })
        .collect();
    for handle in handles {
        handle.join().expect("reader thread");
    }

    assert_eq!(reads.load(Ordering::SeqCst), 1);
}
