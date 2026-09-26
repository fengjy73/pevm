//! Persistent pinned workers.
//!
//! Threads are created the first time a block asks for them and reused for
//! later blocks in this process. They do not carry chain or scheduler state.
//! Each thread pins itself to one CPU from the list the harness passed.
//! A one-worker block does not enter the pool; it runs on the calling thread.

use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex, OnceLock};
use std::thread;

struct Job {
    run: fn(usize, *const ()),
    ctx: *const (),
    /// Workers `0..n` run the job. Higher ids only take part in the barrier.
    n: usize,
}

struct Inner {
    n: usize,
    cpus: Vec<usize>,
    mu: Mutex<()>,
    cv: Condvar,
    epoch: AtomicUsize,
    done: AtomicUsize,
    ready: AtomicUsize,
    job: AtomicPtr<Job>,
    stop: AtomicBool,
}

// The job pointer is published under `mu` and stays valid until every worker
// has incremented `done` and the caller has left `dispatch`.
unsafe impl Sync for Inner {}

struct Pool {
    inner: &'static Inner,
}

fn tls() -> &'static Mutex<Option<Pool>> {
    static POOL: OnceLock<Mutex<Option<Pool>>> = OnceLock::new();
    POOL.get_or_init(|| Mutex::new(None))
}

/// `128-255,4` and comma-separated ids. Empty pieces are skipped.
pub fn parse_cpu_spec(raw: &str) -> Vec<usize> {
    let mut out = Vec::new();
    for part in raw.split(|c: char| c == ',' || c.is_whitespace()) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some((start, end)) = part.split_once('-') {
            let Ok(start) = start.trim().parse::<usize>() else {
                continue;
            };
            let Ok(end) = end.trim().parse::<usize>() else {
                continue;
            };
            if end < start || end - start > 4096 {
                continue;
            }
            out.extend(start..=end);
        } else if let Ok(cpu) = part.parse() {
            out.push(cpu);
        }
    }
    out
}

fn env_cpus() -> Vec<usize> {
    std::env::var("SPECFENCE_PIN_CPUS")
        .ok()
        .map(|raw| parse_cpu_spec(&raw))
        .unwrap_or_default()
}

fn pin_current(cpu: usize) -> bool {
    #[cfg(target_os = "linux")]
    {
        if cpu >= 1024 {
            return false;
        }
        let mut set = [0u8; 128];
        set[cpu / 8] |= 1 << (cpu % 8);
        unsafe extern "C" {
            fn sched_setaffinity(pid: i32, cpusetsize: usize, mask: *const u8) -> i32;
        }
        unsafe { sched_setaffinity(0, 128, set.as_ptr()) == 0 }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = cpu;
        false
    }
}

fn leak_inner(n: usize, cpus: Vec<usize>) -> &'static Inner {
    let inner = Inner {
        n,
        cpus,
        mu: Mutex::new(()),
        cv: Condvar::new(),
        epoch: AtomicUsize::new(0),
        done: AtomicUsize::new(0),
        ready: AtomicUsize::new(0),
        job: AtomicPtr::new(std::ptr::null_mut()),
        stop: AtomicBool::new(false),
    };
    Box::leak(Box::new(inner))
}

fn spawn_workers(inner: &'static Inner) {
    for id in 0..inner.n {
        thread::Builder::new()
            .name(format!("sf-{id}"))
            .spawn(move || worker_main(inner, id))
            .expect("specfence worker");
    }
}

fn worker_main(inner: &'static Inner, id: usize) {
    if let Some(&cpu) = inner.cpus.get(id) {
        let _ = pin_current(cpu);
    }
    inner.ready.fetch_add(1, Ordering::Release);
    {
        let _guard = inner.mu.lock().unwrap();
        inner.cv.notify_all();
    }
    let mut seen = 0usize;
    loop {
        let mut guard = inner.mu.lock().unwrap();
        while inner.epoch.load(Ordering::Acquire) == seen && !inner.stop.load(Ordering::Acquire) {
            guard = inner.cv.wait(guard).unwrap();
        }
        drop(guard);
        if inner.stop.load(Ordering::Acquire) {
            break;
        }
        seen = inner.epoch.load(Ordering::Acquire);
        let job = inner.job.load(Ordering::Acquire);
        if !job.is_null() {
            let job = unsafe { &*job };
            if id < job.n {
                (job.run)(id, job.ctx);
            }
        }
        // Notify under the same mutex the caller waits on, or a completion
        // that lands between the caller's check and its wait is lost.
        let finished = inner.done.fetch_add(1, Ordering::Release) + 1;
        if finished == inner.n {
            let _guard = inner.mu.lock().unwrap();
            inner.cv.notify_all();
        }
    }
}

fn ensure(workers: usize, cpus: &[usize]) -> &'static Inner {
    let mut slot = tls().lock().unwrap();
    let need = workers.max(cpus.len()).max(1);
    if let Some(pool) = slot.as_ref()
        && pool.inner.n >= need
        && (cpus.is_empty() || pool.inner.cpus.len() >= cpus.len())
    {
        return pool.inner;
    }
    // A new list that is longer replaces the pool. The previous threads are
    // left parked until process exit; blocks after this use the larger set.
    let cpus = if cpus.is_empty() {
        slot.as_ref()
            .map(|pool| pool.inner.cpus.clone())
            .unwrap_or_default()
    } else {
        cpus.to_vec()
    };
    let n = need.max(cpus.len());
    let inner = leak_inner(n, cpus);
    spawn_workers(inner);
    let mut guard = inner.mu.lock().unwrap();
    while inner.ready.load(Ordering::Acquire) < n {
        guard = inner.cv.wait(guard).unwrap();
    }
    drop(guard);
    *slot = Some(Pool { inner });
    inner
}

/// Create the threads before the timed loop. Does not run a block.
pub fn prepare(cpus: &[usize], workers: usize) {
    if workers <= 1 && cpus.len() <= 1 {
        return;
    }
    let _ = ensure(workers, cpus);
}

/// Run `f(worker)` on `workers` persistent threads and wait until they return.
///
/// `workers == 1` calls `f(0)` on this thread.
pub(crate) fn dispatch<F>(workers: usize, f: &F)
where
    F: Fn(usize) + Sync,
{
    if workers <= 1 {
        f(0);
        return;
    }
    let cpus = env_cpus();
    let inner = ensure(workers, &cpus);
    // SAFETY: `f` outlives this function. Workers call it only before they
    // increment `done`, and the wait below returns after every worker has.
    let ctx = std::ptr::from_ref(f).cast::<()>();
    let mut job = Job {
        run: call::<F>,
        ctx,
        n: workers,
    };
    let mut guard = inner.mu.lock().unwrap();
    inner.done.store(0, Ordering::Release);
    inner.job.store(&mut job, Ordering::Release);
    inner.epoch.fetch_add(1, Ordering::Release);
    inner.cv.notify_all();
    while inner.done.load(Ordering::Acquire) < inner.n {
        guard = inner.cv.wait(guard).unwrap();
    }
    inner.job.store(std::ptr::null_mut(), Ordering::Release);
    drop(guard);
    let _ = job.n;
}

fn call<F: Fn(usize) + Sync>(id: usize, ctx: *const ()) {
    let f = unsafe { &*(ctx as *const F) };
    f(id);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn parse_ranges_and_ids() {
        assert_eq!(parse_cpu_spec("0-2,5"), vec![0, 1, 2, 5]);
    }

    #[test]
    fn dispatch_reuses_threads() {
        let hits = AtomicUsize::new(0);
        let saw = Mutex::new(vec![false; 3]);
        for _ in 0..2 {
            dispatch(3, &|id| {
                hits.fetch_add(1, Ordering::Relaxed);
                saw.lock().unwrap()[id] = true;
            });
        }
        assert_eq!(hits.load(Ordering::Relaxed), 6);
        assert!(saw.lock().unwrap().iter().all(|hit| *hit));
    }
}
