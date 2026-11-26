//! The Servers list's re-list debounce: one re-list per quiet burst, none while no pane
//! shows the list. Virtual time, so the quiet window costs nothing.

use std::sync::atomic::{AtomicBool, AtomicUsize};

use super::*;

/// A Servers list a test opens and closes, counting its re-lists.
#[derive(Default)]
struct FakeList {
    shown: AtomicBool,
    relists: AtomicUsize,
}

impl ServersList for FakeList {
    fn is_shown(&self) -> bool {
        self.shown.load(Ordering::SeqCst)
    }

    fn relist(&self) {
        self.relists.fetch_add(1, Ordering::SeqCst);
    }
}

fn shown_list() -> (Arc<FakeList>, Arc<Relist>) {
    let list = Arc::new(FakeList::default());
    list.shown.store(true, Ordering::SeqCst);
    let relist = Relist::new(Arc::clone(&list) as Arc<dyn ServersList>);
    (list, relist)
}

/// Asks `relist` for a re-list `times` times, spawning each wait.
fn request(relist: &Arc<Relist>, times: usize) {
    for _ in 0..times {
        if let Some(wait) = relist.request() {
            tokio::spawn(wait);
        }
    }
}

async fn let_it_go_quiet() {
    // allowed-test-sleep: virtual time on a start_paused runtime; the quiet window IS the subject.
    tokio::time::sleep(QUIET * 2).await;
}

/// ❗ A discovery storm (a browse resolving a dozen hosts) is ONE re-list, ❌ never one per
/// event.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_burst_of_changes_is_one_relist() {
    let (list, relist) = shown_list();

    request(&relist, 20);
    let_it_go_quiet().await;

    assert_eq!(list.relists.load(Ordering::SeqCst), 1);
}

/// A change after the list went quiet is a re-list of its own.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_change_after_a_quiet_window_relists_again() {
    let (list, relist) = shown_list();

    request(&relist, 1);
    let_it_go_quiet().await;
    request(&relist, 1);
    let_it_go_quiet().await;

    assert_eq!(list.relists.load(Ordering::SeqCst), 2);
}

/// ❗ With no pane on the Servers list, a change schedules nothing at all.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn nothing_happens_while_no_pane_shows_the_list() {
    let list = Arc::new(FakeList::default());
    let relist = Relist::new(Arc::clone(&list) as Arc<dyn ServersList>);

    assert!(relist.request().is_none(), "nothing to wait for");
    let_it_go_quiet().await;

    assert_eq!(list.relists.load(Ordering::SeqCst), 0);
}

/// A pane that leaves the list during the quiet window gets no re-list built for it.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_list_closed_while_waiting_is_not_relisted() {
    let (list, relist) = shown_list();

    request(&relist, 1);
    list.shown.store(false, Ordering::SeqCst);
    let_it_go_quiet().await;

    assert_eq!(list.relists.load(Ordering::SeqCst), 0);
}
