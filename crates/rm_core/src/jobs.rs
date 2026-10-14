//! Система задач (job system): постоянный пул рабочих потоков.
//!
//! Главная операция — [`JobSystem::parallel_for`]: «выполни `f(0)..f(n-1)`
//! на всех ядрах и дождись». Потоки создаются один раз при старте, а не на
//! каждый кадр, поэтому раздача работы стоит микросекунды.
//!
//! Вызывающий поток тоже участвует в работе, поэтому при `N` ядрах
//! создаётся `N - 1` рабочих потоков.

use std::any::Any;
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, TryLockError};
use std::thread::{self, JoinHandle};

type ErasedFn = *const (dyn Fn(usize) + Sync + 'static);

/// Одна параллельная операция: счётчик следующего индекса и счётчик готовых.
struct Job {
    func: ErasedFn,
    total: usize,
    next: AtomicUsize,
    finished: AtomicUsize,
    panic: Mutex<Option<Box<dyn Any + Send>>>,
    done: Mutex<bool>,
    done_cv: Condvar,
}

// SAFETY: `func` указывает на `Fn + Sync`; вызывается только пока
// `parallel_for` ждёт завершения всех индексов (см. `run`).
unsafe impl Send for Job {}
unsafe impl Sync for Job {}

impl Job {
    /// Забирает индексы, пока они не кончатся. Вызывают и рабочие потоки, и
    /// сам `parallel_for`.
    fn run(&self) {
        loop {
            let i = self.next.fetch_add(1, Ordering::Relaxed);
            if i >= self.total {
                return;
            }
            // SAFETY: индекс i < total ещё не помечен готовым, значит
            // parallel_for ждёт и замыкание живо.
            let func = unsafe { &*self.func };
            if let Err(payload) = panic::catch_unwind(AssertUnwindSafe(|| func(i))) {
                self.panic
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get_or_insert(payload);
            }
            if self.finished.fetch_add(1, Ordering::AcqRel) + 1 == self.total {
                *self.done.lock().unwrap_or_else(|e| e.into_inner()) = true;
                self.done_cv.notify_all();
            }
        }
    }

    fn wait(&self) {
        let mut done = self.done.lock().unwrap_or_else(|e| e.into_inner());
        while !*done {
            done = self.done_cv.wait(done).unwrap_or_else(|e| e.into_inner());
        }
    }
}

#[derive(Default)]
struct State {
    epoch: u64,
    job: Option<Arc<Job>>,
    shutdown: bool,
}

#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    wake: Condvar,
}

/// Пул потоков. Обычно один на всю игру.
pub struct JobSystem {
    shared: Arc<Shared>,
    workers: Vec<JoinHandle<()>>,
    /// Одновременно выполняется одна параллельная операция; вложенные или
    /// конкурирующие вызовы выполняются последовательно в своём потоке.
    busy: Mutex<()>,
}

impl JobSystem {
    /// Пул на все ядра машины (рабочих потоков = ядер − 1).
    pub fn new() -> Self {
        let cores = thread::available_parallelism().map_or(1, |n| n.get());
        Self::with_threads(cores)
    }

    /// Пул, в котором работу делают `threads` потоков, включая вызывающий.
    /// `with_threads(1)` — всё выполняется последовательно (удобно для отладки).
    pub fn with_threads(threads: usize) -> Self {
        let shared = Arc::new(Shared::default());
        let workers = (1..threads.max(1))
            .map(|n| {
                let shared = Arc::clone(&shared);
                thread::Builder::new()
                    .name(format!("rm-worker-{n}"))
                    .spawn(move || worker_loop(&shared))
                    .expect("не удалось создать рабочий поток")
            })
            .collect();
        Self {
            shared,
            workers,
            busy: Mutex::new(()),
        }
    }

    /// Сколько потоков делают работу (включая вызывающий).
    pub fn threads(&self) -> usize {
        self.workers.len() + 1
    }

    /// Выполняет `f(i)` для каждого `i` в `0..n` параллельно и возвращается,
    /// когда все вызовы завершены. Паника внутри `f` пробрасывается сюда.
    ///
    /// Порядок вызовов не определён. Чтобы результат был детерминированным,
    /// каждый `f(i)` должен писать только в «свои» данные.
    pub fn parallel_for<F: Fn(usize) + Sync>(&self, n: usize, f: F) {
        if n == 0 {
            return;
        }
        let guard = match self.busy.try_lock() {
            Ok(guard) => Some(guard),
            Err(TryLockError::Poisoned(e)) => Some(e.into_inner()),
            Err(TryLockError::WouldBlock) => None,
        };
        if guard.is_none() || self.workers.is_empty() || n == 1 {
            (0..n).for_each(f);
            return;
        }

        let func: &(dyn Fn(usize) + Sync) = &f;
        // SAFETY: стираем время жизни замыкания. Мы не вернёмся из функции,
        // пока все n вызовов не завершатся (job.wait()), а после этого никто
        // замыкание не трогает: индексы >= total не вызывают func.
        let func: ErasedFn =
            unsafe { std::mem::transmute::<*const (dyn Fn(usize) + Sync + '_), ErasedFn>(func) };
        let job = Arc::new(Job {
            func,
            total: n,
            next: AtomicUsize::new(0),
            finished: AtomicUsize::new(0),
            panic: Mutex::new(None),
            done: Mutex::new(false),
            done_cv: Condvar::new(),
        });
        {
            let mut state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
            state.epoch += 1;
            state.job = Some(Arc::clone(&job));
        }
        self.shared.wake.notify_all();

        job.run();
        job.wait();

        self.shared
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .job = None;
        drop(guard);
        let payload = job.panic.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(payload) = payload {
            panic::resume_unwind(payload);
        }
    }

    /// Как `parallel_for`, но `f` получает диапазон индексов длиной до `batch`.
    /// Удобно, когда работа на один индекс слишком мелкая.
    pub fn parallel_for_batched<F: Fn(std::ops::Range<usize>) + Sync>(
        &self,
        n: usize,
        batch: usize,
        f: F,
    ) {
        let batch = batch.max(1);
        self.parallel_for(n.div_ceil(batch), |i| {
            let start = i * batch;
            f(start..(start + batch).min(n));
        });
    }

    /// Выполняет две независимые задачи параллельно и возвращает оба результата.
    pub fn join<A, B, RA, RB>(&self, a: A, b: B) -> (RA, RB)
    where
        A: FnOnce() -> RA + Send,
        B: FnOnce() -> RB + Send,
        RA: Send,
        RB: Send,
    {
        let a = Mutex::new(Some(a));
        let b = Mutex::new(Some(b));
        let ra = Mutex::new(None);
        let rb = Mutex::new(None);
        self.parallel_for(2, |i| {
            if i == 0 {
                let f = a.lock().unwrap().take().unwrap();
                *ra.lock().unwrap() = Some(f());
            } else {
                let f = b.lock().unwrap().take().unwrap();
                *rb.lock().unwrap() = Some(f());
            }
        });
        (
            ra.into_inner().unwrap().unwrap(),
            rb.into_inner().unwrap().unwrap(),
        )
    }
}

impl Default for JobSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for JobSystem {
    fn drop(&mut self) {
        self.shared
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .shutdown = true;
        self.shared.wake.notify_all();
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

fn worker_loop(shared: &Shared) {
    let mut seen_epoch = 0;
    loop {
        let job = {
            let mut state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
            while state.epoch == seen_epoch && !state.shutdown {
                state = shared.wake.wait(state).unwrap_or_else(|e| e.into_inner());
            }
            if state.shutdown {
                return;
            }
            seen_epoch = state.epoch;
            state.job.clone()
        };
        if let Some(job) = job {
            job.run();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parallel_for_visits_every_index_once() {
        let jobs = JobSystem::with_threads(4);
        let hits: Vec<AtomicUsize> = (0..10_000).map(|_| AtomicUsize::new(0)).collect();
        for _ in 0..50 {
            jobs.parallel_for(hits.len(), |i| {
                hits[i].fetch_add(1, Ordering::Relaxed);
            });
        }
        assert!(hits.iter().all(|h| h.load(Ordering::Relaxed) == 50));
    }

    #[test]
    fn batched_covers_range() {
        let jobs = JobSystem::with_threads(3);
        let sum = AtomicUsize::new(0);
        jobs.parallel_for_batched(1001, 64, |r| {
            sum.fetch_add(r.sum::<usize>(), Ordering::Relaxed);
        });
        assert_eq!(sum.into_inner(), (0..1001).sum::<usize>());
    }

    #[test]
    fn join_returns_both() {
        let jobs = JobSystem::with_threads(2);
        let (a, b) = jobs.join(|| 2 + 2, || "b".to_string());
        assert_eq!((a, b.as_str()), (4, "b"));
    }

    #[test]
    fn nested_parallel_for_does_not_deadlock() {
        let jobs = JobSystem::with_threads(4);
        let count = AtomicUsize::new(0);
        jobs.parallel_for(8, |_| {
            jobs.parallel_for(8, |_| {
                count.fetch_add(1, Ordering::Relaxed);
            });
        });
        assert_eq!(count.into_inner(), 64);
    }

    #[test]
    fn panic_is_propagated_and_pool_survives() {
        let jobs = JobSystem::with_threads(4);
        let result = panic::catch_unwind(AssertUnwindSafe(|| {
            jobs.parallel_for(100, |i| assert!(i != 57, "boom"));
        }));
        assert!(result.is_err());
        let count = AtomicUsize::new(0);
        jobs.parallel_for(100, |_| {
            count.fetch_add(1, Ordering::Relaxed);
        });
        assert_eq!(count.into_inner(), 100);
    }
}
