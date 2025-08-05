use std::{
    panic,
    sync::{
        atomic::{ AtomicBool, Ordering },
        mpsc::{ self, RecvTimeoutError },
        Arc,
        Mutex,
        RwLock,
    },
    thread,
    time::Duration,
};

// JOB/Function that we will run through threads
type Job = Box<dyn FnOnce() + Send + 'static>;

pub struct WorkerPool {
    workers: Arc<RwLock<Vec<Worker>>>,
    sender: Option<mpsc::Sender<Job>>,
    shutdown_master: Arc<AtomicBool>,
}

impl WorkerPool {
    /**
     * Create workerpool with specific size
     */
    pub fn new(size: usize) -> WorkerPool {
        // Size must be greater than 0
        assert!(size > 0);
        // Initialize thread channle
        let (sender, receiver) = mpsc::channel();
        // Wrap receiver with Atomic refercounter
        let receiver = Arc::new(Mutex::new(receiver));

        let workers = Arc::new(RwLock::new(Vec::with_capacity(size)));

        // Block scope initialize all workers
        {
            let binding = Arc::clone(&workers);
            let mut write_worker = binding.write().unwrap();
            for id in 0..size {
                write_worker.push(Worker::new(id, Arc::clone(&receiver)));
            }
        }
        // AtomicBool for drop master thread
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        // Clone for move it into thread
        let shutdown_clone = Arc::clone(&shutdown_flag);
        let worker_clone = Arc::clone(&workers);
        let receiver_clone = Arc::clone(&receiver);

        // Run master thread
        thread::spawn(move || {
            loop {
                // Catch if master thread panics
                let result = panic::catch_unwind({
                    let shutdown_flag = Arc::clone(&shutdown_clone);
                    let worker_clone = Arc::clone(&worker_clone);
                    let receiver_clone = Arc::clone(&receiver_clone);

                    move || {
                        // While workerpool in scope
                        while !shutdown_flag.load(Ordering::Relaxed) {
                            // Check dead worker every 4s
                            thread::sleep(Duration::from_secs(4));
                            // Find all dead workers
                            let dead_worker_id: Vec<usize> = {
                                // Safely gets read lock
                                let workers = match worker_clone.read() {
                                    Ok(w) => w,
                                    Err(e) => {
                                        eprintln!("Watchdog lock poisoned: {e}");
                                        continue;
                                    }
                                };
                                // Filter out all dead workers
                                workers
                                    .iter()
                                    .filter(|w| {
                                        w.thread
                                            .as_ref()
                                            .map(|t| t.is_finished())
                                            .unwrap_or(true)
                                    })
                                    .map(|w| w.id)
                                    .collect()
                            };

                            // Replace with new workers
                            for id in dead_worker_id {
                                let new_worker = Worker::new(id, Arc::clone(&receiver_clone));

                                if let Ok(mut workers) = worker_clone.write() {
                                    if let Some(slot) = workers.iter_mut().find(|w| w.id == id) {
                                        *slot = new_worker;
                                    }
                                } else {
                                    eprintln!("Can't get worker write lock");
                                }
                            }
                        }
                    }
                });
                // Panic master thread restart it
                if result.is_err() {
                    eprintln!("Master thread panicked. Restarting master thread");
                    thread::sleep(Duration::from_secs(1));
                    continue;
                } else {
                    // Case workepool drop
                    break;
                }
            }
        });

        WorkerPool {
            workers,
            sender: Some(sender),
            shutdown_master: shutdown_flag,
        }
    }

    // Execute jobs
    pub fn execute<F>(&self, f: F) where F: FnOnce() + Send + 'static {
        let job = Box::new(f);

        if let Some(sender) = &self.sender {
            if sender.send(job).is_err() {
                eprintln!("Failed to send job: worker pool might be shutting down.");
            }
        }
    }
}

// Implement drop
impl Drop for WorkerPool {
    fn drop(&mut self) {
        self.shutdown_master.store(true, Ordering::Relaxed);

        drop(self.sender.take());

        let mut workers = self.workers.write().unwrap();

        for worker in workers.iter_mut() {
            println!("Shutting down worker {}", worker.id);

            if let Some(thread) = worker.thread.take() {
                thread.join().unwrap();
            }
        }
    }
}

struct Worker {
    id: usize,
    thread: Option<thread::JoinHandle<()>>,
}

impl Worker {
    // Create new worker
    fn new(id: usize, receiver: Arc<Mutex<mpsc::Receiver<Job>>>) -> Worker {
        let thread = thread::spawn(move || {
            loop {
                let message = receiver.lock().unwrap().recv_timeout(Duration::from_secs(1));

                match message {
                    Ok(job) => {
                        let result = panic::catch_unwind(panic::AssertUnwindSafe(job));
                        if result.is_err() {
                            eprintln!("Worker {id} panicked while executing a job.");
                        }
                    }
                    Err(RecvTimeoutError::Timeout) => {
                        continue;
                    }
                    Err(_) => {
                        eprintln!("Worker {id} disconnected; shutting down.");
                        break;
                    }
                }
            }
        });

        Worker {
            id,
            thread: Some(thread),
        }
    }
}
