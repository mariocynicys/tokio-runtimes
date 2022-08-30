#![allow(unused_imports, non_snake_case, dead_code)]

enum TaskType {
    Sync,
    Async,
}

const TASK_TYPE: TaskType = TaskType::Async;

async fn inner_async(task_num: u32) {
    println!("Starting task #{task_num}");
    let before = std::time::Instant::now();
    match TASK_TYPE {
        TaskType::Sync => {
            // Simulate a sync sleep (blocking). This is about 26ms busy waiting in normal cases.
            let mut m = 10000000;
            while m != 0 {
                m -= 1;
            }
        }
        TaskType::Async => {
            // Simulate an async task that takes about 26 milli seconds to finish.
            tokio::time::sleep(tokio::time::Duration::from_millis(26)).await;
        }
    }
    let after = std::time::Instant::now();
    println!("Finished task #{task_num} took {:?}", after - before);
}

/// Runs `inner_async` inside a `block_on`.
fn outer_sync_using_block_on(task_num: u32) {
    tokio::runtime::Handle::current().block_on(inner_async(task_num));
}

/// Runs `inner_async` inside a `block_on` using the provided runtime.
fn outer_sync_using_runtime(task_num: u32, rt: &tokio::runtime::Runtime) {
    rt.block_on(inner_async(task_num));
}

/// Runs `inner_async` inside a `block_on` using the provided handle.
fn outer_sync_using_handle(task_num: u32, handle: &tokio::runtime::Handle) {
    handle.block_on(inner_async(task_num));
}

#[cfg(test)]
mod tests {
    use super::*;

    const N_REQUESTS: u32 = 11000;

    /// DEADLOCKS
    /// spawning tokio tasks and block in place inside them and await these spawned tasks.
    #[tokio::test(flavor = "multi_thread")]
    async fn test01() {
        let mut tasks = Vec::new();

        for i in 0..N_REQUESTS {
            let handle = tokio::runtime::Handle::current();
            let i = i;
            tasks.push(tokio::spawn(async move {
                // Since `block_on` can not be ran from an async context,
                // we have to wrap it inside `block_in_place`.
                // Note that `block_in_place` needs to run in a multi-threaded runtime.
                tokio::task::block_in_place(|| {
                    //outer_sync_using_block_on(i);
                    outer_sync_using_handle(i, &handle)
                })
            }))
        }

        println!("Awaiting on the tasks");
        // Awaiting on finished tasks allows other tasks to move on.
        // Many tasks (not all) will start but few only will finish.
        for task in tasks {
            task.await.unwrap();
        }
    }

    /// DEADLOCKS
    /// spawning tokio tasks and block in place inside them but don't await these spawned tasks.
    #[tokio::test(flavor = "multi_thread")]
    async fn test2() {
        let mut tasks = Vec::new();

        for i in 0..N_REQUESTS {
            let i = i;
            tasks.push(tokio::spawn(async move {
                // Since `block_on` can not be ran from an async context,
                // we have to wrap it inside `block_in_place`.
                // Note that `block_in_place` needs to run in a multi-threaded runtime.
                tokio::task::block_in_place(|| {
                    outer_sync_using_block_on(i);
                })
            }))
        }

        println!("Spawned all the tasks");
        // Due to not awaiting on the tasks, only about 30 tasks will start and never finish.
    }

    /// DEADLOCKS
    /// spawning tokio tasks and block in place inside them and await only on finished tasks.
    #[tokio::test(flavor = "multi_thread")]
    async fn test3() {
        let mut tasks = Vec::new();

        for i in 0..N_REQUESTS {
            let i = i;
            tasks.push(tokio::spawn(async move {
                // Since `block_on` can not be ran from an async context,
                // we have to wrap it inside `block_in_place`.
                // Note that `block_in_place` needs to run in a multi-threaded runtime.
                tokio::task::block_in_place(|| {
                    outer_sync_using_block_on(i);
                })
            }))
        }

        println!("Polling finished tasks and awaiting on them");
        // This out performs test1 by a little bit.
        let mut tasks_left = tasks.len();
        while tasks_left != 0 {
            for i in 0..tasks_left {
                if tasks[i].is_finished() {
                    let task = tasks.remove(i);
                    task.await.unwrap();
                    tasks_left -= 1;
                    break;
                }
            }
        }
    }

    /// DEADLOCKS
    /// used `futures::executor::block_on` but this caped at 8 tasks and deadlocked.
    #[tokio::test]
    async fn test4() {
        // missing
    }

    /// avg ~0.7s
    /// spawning blocking tokio tasks and run the `block_on` tasks directly inside and await these spawned tasks.
    #[tokio::test]
    async fn test5() {
        let mut tasks = Vec::new();

        for i in 0..N_REQUESTS {
            let i = i;
            tasks.push(tokio::task::spawn_blocking(move || {
                // // The `block_in_place` isn't just there at all in this sync context.
                // // No performance degradation.
                // tokio::task::block_in_place(|| {
                //     outer_sync_using_block_on(i);
                // });
                outer_sync_using_block_on(i);
            }))
        }

        println!("Awaiting on the tasks");
        for task in tasks {
            task.await.unwrap();
        }
    }

    /// avg ~13s (panics with many number of tasks)
    /// spawning `std::thread`s and build a runtime inside them to carry out the task.
    #[tokio::test]
    async fn test6() {
        let mut tasks = Vec::new();

        for i in 0..N_REQUESTS {
            let i = i;
            tasks.push(std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().unwrap();
                outer_sync_using_runtime(i, &rt);
            }))
        }

        println!("Awaiting on the tasks");
        for task in tasks {
            task.join().unwrap();
        }
    }

    /// avg ~270s
    /// uses only one worker thread and a channel to send tasks.
    #[tokio::test]
    async fn test7() {
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            let timeout_dur = std::time::Duration::from_millis(10);
            let mut finished = 0;
            while finished != N_REQUESTS {
                if let Ok(i) = rx.recv_timeout(timeout_dur) {
                    outer_sync_using_runtime(i, &rt);
                    finished += 1;
                }
            }
        });

        for i in 0..N_REQUESTS {
            let i = i;
            tx.send(i).unwrap();
        }

        worker.join().unwrap();
    }

    /// avg ~10s (when number of threads is 500)
    /// uses `threadpool::Threadpool`.
    #[tokio::test]
    async fn test8() {
        let pool = threadpool::ThreadPool::new(500);

        for i in 0..N_REQUESTS {
            let i = i;
            pool.execute(move || {
                let rt = tokio::runtime::Runtime::new().unwrap();
                outer_sync_using_runtime(i, &rt);
            })
        }

        pool.join();
    }

    /// avg ~0.7s (when number of threads is 500)
    /// uses `threadpool::Threadpool` but sharing a single runtime.
    #[tokio::test]
    async fn test9() {
        let pool = threadpool::ThreadPool::new(500);
        // We have the runtime inside a box and leaking it here because it can't be dropped
        // inside another runtime `tokio::test` as this will panic when the test finishes.
        let rt = Box::leak(Box::new(tokio::runtime::Runtime::new().unwrap()));

        for i in 0..N_REQUESTS {
            let handle = rt.handle().clone();
            let i = i;
            pool.execute(move || {
                outer_sync_using_handle(i, &handle);
            })
        }

        pool.join();
    }

    /// avg ~0.7s
    /// builds two runtimes, one used as a global runtime and
    /// another used as a thread pool to run blocking tasks on.
    #[test]
    fn test10() {
        let thread_pool = tokio::runtime::Runtime::new().unwrap();
        let handle = thread_pool.handle().clone();
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async move {
                let mut tasks = Vec::new();

                for i in 0..N_REQUESTS {
                    let i = i;
                    let handle = handle.clone();
                    tasks.push(tokio::task::spawn(async move {
                        tokio::task::block_in_place(|| {
                            outer_sync_using_handle(i, &handle);
                        });
                    }))
                }

                println!("Awaiting on the tasks");
                for task in tasks {
                    task.await.unwrap();
                }
            });
    }

    /// avg ~0.7s
    /// a `test10` rewrite that creates the thread pool inside the global runtime.
    /// needs to be a "multi_thread" flavor because `block_in_place`.
    #[tokio::test(flavor = "multi_thread")]
    async fn test11() {
        // We have the runtime inside a box and leaking it here because it can't be dropped
        // inside another runtime `tokio::test` as this will panic when the test finishes.
        let thread_pool = Box::leak(Box::new(tokio::runtime::Runtime::new().unwrap()));
        let mut tasks = Vec::new();

        for i in 0..N_REQUESTS {
            let i = i;
            let handle = thread_pool.handle().clone();
            tasks.push(tokio::task::spawn(async move {
                tokio::task::block_in_place(|| {
                    outer_sync_using_handle(i, &handle);
                });
            }))
        }

        println!("Awaiting on the tasks");
        for task in tasks {
            task.await.unwrap();
        }
    }

    // NOTE: `#[tokio::test(flavor = "multi_thread")]` is equivalent to `#[tokio::main]#[test]`.
}
