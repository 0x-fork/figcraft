//! Canvas rendering off the UI thread (desktop) or inline (web). The latest request wins; the UI
//! keeps showing the previous image, re-projected to the current view, until the new one arrives.

use std::sync::Arc;

use figcraft_doc::{Document, NodeId};
use figcraft_render::View;

#[derive(Clone)]
pub struct Job {
    pub doc: Arc<Document>,
    pub page: NodeId,
    pub view: View,
    pub outline: bool,
    pub skip: Vec<NodeId>,
    pub generation: u64,
}

pub struct Done {
    pub rgba: Vec<u8>,
    pub view: View,
    pub generation: u64,
    pub ms: f64,
}

fn run(job: &Job) -> Option<Done> {
    #[cfg(not(target_arch = "wasm32"))]
    let t0 = std::time::Instant::now();
    let opts = figcraft_render::Options { outline_mode: job.outline, skip: job.skip.clone(), ..Default::default() };
    let pix = figcraft_render::render_view(&job.doc, job.page, &job.view, &opts)?;
    #[cfg(not(target_arch = "wasm32"))]
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    #[cfg(target_arch = "wasm32")]
    let ms = 0.0;
    Some(Done { rgba: pix.data().to_vec(), view: job.view.clone(), generation: job.generation, ms })
}

#[cfg(not(target_arch = "wasm32"))]
pub struct Worker {
    tx: std::sync::mpsc::Sender<Job>,
    rx: std::sync::mpsc::Receiver<Done>,
    pub in_flight: bool,
}

#[cfg(not(target_arch = "wasm32"))]
impl Worker {
    pub fn new(ctx: egui::Context) -> Self {
        let (tx, jobs) = std::sync::mpsc::channel::<Job>();
        let (done_tx, rx) = std::sync::mpsc::channel::<Done>();
        let spawned = std::thread::Builder::new().name("figcraft-render".into()).spawn(move || {
            while let Ok(mut job) = jobs.recv() {
                // Skip to the newest queued job.
                while let Ok(newer) = jobs.try_recv() {
                    job = newer;
                }
                let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(&job)));
                if let Ok(Some(d)) = res
                    && done_tx.send(d).is_err()
                {
                    break;
                }
                ctx.request_repaint();
            }
        });
        if let Err(e) = spawned {
            log::error!("render thread: {e}");
        }
        Self { tx, rx, in_flight: false }
    }

    pub fn submit(&mut self, job: Job) {
        if self.tx.send(job).is_ok() {
            self.in_flight = true;
        }
    }

    pub fn poll(&mut self) -> Option<Done> {
        let mut last = None;
        while let Ok(d) = self.rx.try_recv() {
            last = Some(d);
        }
        if last.is_some() {
            self.in_flight = false;
        }
        last
    }
}

#[cfg(target_arch = "wasm32")]
pub struct Worker {
    pending: Option<Job>,
    pub in_flight: bool,
}

#[cfg(target_arch = "wasm32")]
impl Worker {
    pub fn new(_ctx: egui::Context) -> Self {
        Self { pending: None, in_flight: false }
    }
    pub fn submit(&mut self, job: Job) {
        self.pending = Some(job);
        self.in_flight = true;
    }
    pub fn poll(&mut self) -> Option<Done> {
        let job = self.pending.take()?;
        self.in_flight = false;
        run(&job)
    }
}

/// Render synchronously (tests, screenshots).
pub fn render_now(job: &Job) -> Option<Done> {
    run(job)
}
