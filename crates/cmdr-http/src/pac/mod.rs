//! PAC files (and so WPAD), run by the OS itself: CFNetwork's `Execute…` calls on macOS,
//! WinHTTP's `WinHttpGetProxyForUrl` on Windows (`platform/`). What's shared lives here: the
//! per-host answer cache, the one-evaluation-at-a-time turn, the budgets, and the warm-up.
//!
//! Each OS's evaluation can block for seconds (fetching the file, WPAD's discovery), and reqwest
//! asks synchronously, so each cache miss runs on a short-lived thread of its own and the asking
//! thread waits at most [`TIMEOUT`]. Answers are cached per PAC and per `scheme://host:port`.

pub(crate) mod platform;

use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use reqwest::Url;

use crate::Route;
use crate::entries::{Entry, Pac, PacSource};
use crate::env::EnvProxies;

/// How long the asking thread waits for a PAC answer before going direct, the way macOS does
/// when a PAC can't be run. Covers fetching the file, which the OS does itself.
pub(crate) const TIMEOUT: Duration = Duration::from_secs(3);
/// The launch warm-up's budget ([`crate::warm_up_pac`]): a process's first evaluation is the
/// cold one (0.2–2 s on a busy Mac, CFNetwork trying TLS twice on an `http://` PAC URL first),
/// and paying it here, off any request, keeps a loaded Mac from missing [`TIMEOUT`] and going
/// direct for [`FAILURE_TTL`] (#810). A request arriving meanwhile waits for this answer.
pub(crate) const WARM_UP_TIMEOUT: Duration = Duration::from_secs(10);
/// The budget for a test cell that wants the PAC's ANSWER, ❌ not one about [`TIMEOUT`]: each
/// test process pays the OS's cold first evaluation (0.2–2 s on a busy Mac, then about 1 ms
/// warm), so under load `TIMEOUT` tripped and the walk went direct (#754). Past it, the nextest
/// cap reports a starved cell as a timeout, which is what it is.
#[cfg(test)]
pub(crate) const ANSWER_BUDGET: Duration = Duration::from_secs(60);
/// How long an answer stays good. A PAC file rarely changes, and this bounds how long a changed
/// one takes to apply.
const ANSWER_TTL: Duration = Duration::from_secs(5 * 60);
/// How long a failure stays cached, so an unreachable PAC costs one [`TIMEOUT`] per host per
/// half minute rather than one per request.
const FAILURE_TTL: Duration = Duration::from_secs(30);
/// Entries kept before the cache starts over: a few dozen hosts in normal use.
const CACHE_CAP: usize = 256;

/// The process-wide evaluator [`crate::client_builder`]'s clients share.
pub(crate) fn shared() -> &'static OsPac {
    static SHARED: LazyLock<OsPac> = LazyLock::new(|| OsPac::new(TIMEOUT));
    &SHARED
}

type Key = (PacSource, String);
/// An answer (`None` for a failure) and when it was had.
type Cached = (Option<Vec<Entry>>, Instant);

/// The OS's PAC evaluator with a per-host answer cache.
pub(crate) struct OsPac {
    timeout: Duration,
    cache: Mutex<HashMap<Key, Cached>>,
    /// One evaluation at a time: a burst of first connections to one host (an S3 upload's parts)
    /// waits for the first answer instead of running the same PAC N times.
    turn: Mutex<()>,
    /// Evaluations run so far, so a test can see the cache answer. The OS keeps a fetched PAC
    /// file and can fetch it again after answering, so neither its answers nor the server's hits
    /// tell a cache hit from a miss.
    #[cfg(test)]
    evaluations: std::sync::atomic::AtomicUsize,
}

impl OsPac {
    pub(crate) fn new(timeout: Duration) -> Self {
        Self {
            timeout,
            cache: Mutex::new(HashMap::new()),
            turn: Mutex::new(()),
            #[cfg(test)]
            evaluations: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    fn cached(&self, key: &Key) -> Option<Option<Vec<Entry>>> {
        let cache = self.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let (answer, at) = cache.get(key)?;
        let ttl = if answer.is_some() { ANSWER_TTL } else { FAILURE_TTL };
        (at.elapsed() < ttl).then(|| answer.clone())
    }

    fn store(&self, key: Key, answer: Option<Vec<Entry>>) {
        let mut cache = self.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if cache.len() >= CACHE_CAP {
            cache.clear();
        }
        cache.insert(key, (answer, Instant::now()));
    }

    /// This evaluator with its own cache and turn, but `budget` in place of its timeout for a
    /// miss. A request asking meanwhile waits for that turn and then reads the answer.
    pub(crate) fn with_budget(&self, budget: Duration) -> Budgeted<'_> {
        Budgeted { pac: self, budget }
    }

    fn evaluate_within(&self, source: &PacSource, url: &Url, timeout: Duration) -> Option<Vec<Entry>> {
        let key = (source.clone(), url.as_str().to_string());
        if let Some(answer) = self.cached(&key) {
            return answer;
        }
        let _turn = self.turn.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(answer) = self.cached(&key) {
            return answer;
        }
        #[cfg(test)]
        self.evaluations.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let answer = evaluate_on_own_thread(source.clone(), url.as_str().to_string(), timeout);
        if answer.is_none() {
            // allowed-log-private: cmdr-http can't depend on cmdr-fs, and a keyed `host="…"` is redacted exactly
            log::warn!(target: "cmdr_http", "couldn't run the PAC file for host={:?}, so it goes direct", url.host_str().unwrap_or_default());
        }
        self.store(key, answer.clone());
        answer
    }
}

impl Pac for OsPac {
    fn evaluate(&self, source: &PacSource, url: &Url) -> Option<Vec<Entry>> {
        self.evaluate_within(source, url, self.timeout)
    }
}

/// An [`OsPac`] whose misses get a budget of their own: the launch warm-up's.
pub(crate) struct Budgeted<'a> {
    pac: &'a OsPac,
    budget: Duration,
}

impl Pac for Budgeted<'_> {
    fn evaluate(&self, source: &PacSource, url: &Url) -> Option<Vec<Entry>> {
        self.pac.evaluate_within(source, url, self.budget)
    }
}

/// Runs one evaluation on a fresh thread and waits for it, at most `timeout` plus a margin for
/// the thread to notice its own deadline. A thread that outlives the wait finishes on its own and
/// its answer goes nowhere.
fn evaluate_on_own_thread(source: PacSource, url: String, timeout: Duration) -> Option<Vec<Entry>> {
    let (sender, receiver) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name(String::from("cmdr-http-pac"))
        .spawn(move || {
            // The receiver may have given up already; nothing else to tell.
            let _ = sender.send(platform::execute(&source, &url, timeout));
        });
    if spawned.is_err() {
        return None;
    }
    receiver.recv_timeout(timeout + Duration::from_secs(1)).ok().flatten()
}

/// An OS arm's whole system answer for a URL, with a PAC entry run by the given evaluator.
pub(crate) type RouteWith = fn(&Url, &dyn Pac) -> Route;

/// The system layer [`warm_up`] decides with: the arm's route, but a PAC miss gets
/// [`WARM_UP_TIMEOUT`], since nothing waits on it but the requests that would otherwise have run
/// the cold first evaluation themselves.
struct WarmingSystem(RouteWith);

impl crate::SystemProxies for WarmingSystem {
    fn route(&self, url: &Url) -> Route {
        (self.0)(url, &shared().with_budget(WARM_UP_TIMEOUT))
    }
}

/// Runs the PAC evaluation for `url` once, on a background thread, through `route_with`.
pub(crate) fn warm_up(url: Url, env: Arc<EnvProxies>, route_with: RouteWith) {
    let spawned = std::thread::Builder::new()
        .name(String::from("cmdr-http-pac-warm-up"))
        .spawn(move || {
            // The answer lands in the PAC cache; the route itself is nobody's to act on.
            let _ = crate::route::decide(&url, &env, &WarmingSystem(route_with));
        });
    if spawned.is_err() {
        log::debug!(target: "cmdr_http", "couldn't start the PAC warm-up; the first request runs it instead");
    }
}

#[cfg(test)]
pub(crate) mod test_server;

#[cfg(test)]
#[path = "pac_test.rs"]
mod tests;
