use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::{ Arc, Mutex };
use std::task::{ Context, Poll, RawWaker, RawWakerVTable, Waker };

// A task is just a pinned box Future
pub struct Task {
    future: Mutex<Pin<Box<dyn Future<Output = ()> + Send>>>,
}

impl Task {
    pub fn new(fut: impl Future<Output = ()> + Send + 'static) -> Arc<Self> {
        Arc::new(Task {
            future: Mutex::new(Box::pin(fut)),
        })
    }

    pub fn poll(self: Arc<Self>) {
        let waker = make_waker(self.clone());
        let mut ctx = Context::from_waker(&waker);
        let mut future = self.future.lock().unwrap();
        let _ = future.as_mut().poll(&mut ctx);
    }
}

// Minimal executor
pub struct Executor {
    tasks: Arc<Mutex<VecDeque<Arc<Task>>>>,
}

impl Executor {
    pub fn new() -> Self {
        Self {
            tasks: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    pub fn spawn(&self, task: Arc<Task>) {
        self.tasks.lock().unwrap().push_back(task);
    }

    pub fn run(&self) {
        while let Some(task) = self.tasks.lock().unwrap().pop_front() {
            task.poll();
        }
    }
}

// Make a Waker from a Task
fn make_waker(task: Arc<Task>) -> Waker {
    unsafe fn clone(ptr: *const ()) -> RawWaker {
        let arc = Arc::<Task>::from_raw(ptr as *const Task);
        std::mem::forget(arc.clone());
        RawWaker::new(ptr, &VTABLE)
    }

    unsafe fn wake(ptr: *const ()) {
        let task = Arc::<Task>::from_raw(ptr as *const Task);
        task.poll();
    }

    unsafe fn wake_by_ref(ptr: *const ()) {
        let arc = Arc::<Task>::from_raw(ptr as *const Task);
        arc.clone().poll();
        std::mem::forget(arc);
    }

    unsafe fn drop(_: *const ()) {}

    static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake_by_ref, drop);
    let raw = RawWaker::new(Arc::into_raw(task) as *const (), &VTABLE);
    unsafe { Waker::from_raw(raw) }
}

use std::{ task::{ RawWaker, RawWakerVTable, Waker } };

mod futures_waker {
    use std::{ sync::{ Arc, mpsc::Sender }, task::{ RawWaker, RawWakerVTable, Waker } };
    use super::Task;

    pub fn waker(task: Arc<Task>) -> Waker {
        unsafe { Waker::from_raw(raw_waker(task)) }
    }

    fn raw_waker(task: Arc<Task>) -> RawWaker {
        fn clone(ptr: *const ()) -> RawWaker {
            let arc = unsafe { Arc::<Task>::from_raw(ptr as *const Task) };
            let cloned = arc.clone();
            std::mem::forget(arc); // Don't decrease ref count
            raw_waker(cloned)
        }

        fn wake(ptr: *const ()) {
            let task = unsafe { Arc::<Task>::from_raw(ptr as *const Task) };
            task.poll(); // re-poll
        }

        fn wake_by_ref(ptr: *const ()) {
            let task = unsafe { Arc::<Task>::from_raw(ptr as *const Task) };
            task.clone().poll(); // re-poll
            std::mem::forget(task);
        }

        fn drop(ptr: *const ()) {
            unsafe { Arc::<Task>::from_raw(ptr as *const Task) }
        }

        let vtable = &RawWakerVTable::new(clone, wake, wake_by_ref, drop);
        RawWaker::new(Arc::into_raw(task) as *const (), vtable)
    }
}

pub type Handler = Arc<
    dyn (Fn(Arc<Request>, Arc<RwLock<Response>>) -> Pin<Box<dyn Future<Output = ()> + Send>>) +
        Send +
        Sync
>;

// executor.spawn(handler(req.clone(), res.clone()));

fn main() {
    let executor = Executor::new();

    executor.spawn(async {
        println!("Task 1 sleeping...");
        std::thread::sleep(std::time::Duration::from_secs(1));
        println!("Task 1 done.");
    });

    executor.spawn(async {
        println!("Task 2 running.");
    });

    executor.run(); // Runs forever unless you break
}
