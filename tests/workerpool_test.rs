// use std::{ sync::{ atomic::{ AtomicUsize, Ordering }, Arc }, thread, time::Duration };

// use glote::workerpool::WorkerPool;

// #[test]
// fn test_workerpool() {
//     let counter = Arc::new(AtomicUsize::new(0));
//     let pool = WorkerPool::new(4);

//     let job_count = 8;

//     for i in 0..job_count {
//         let c = Arc::clone(&counter);
//         pool.execute(move || {
//             println!("Running job {i}");
//             c.fetch_add(1, Ordering::Relaxed);
//             thread::sleep(Duration::from_millis(100));
//         });
//     }

//     thread::sleep(Duration::from_secs(5));

//     drop(pool);

//     assert_eq!(counter.load(Ordering::Relaxed), job_count, "All jobs have completed");
// }
