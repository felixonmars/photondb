use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Instant,
};

use clap::Parser;
use photondb_engine::tree::{Map, Options};
use rand::{
    distributions::{Distribution, Uniform, WeightedIndex},
    rngs::ThreadRng,
};

#[derive(Parser, Debug)]
struct Config {
    #[clap(long, default_value = "1000000")]
    pub num_kvs: u64,
    #[clap(long, default_value = "8")]
    pub key_len: usize,
    #[clap(long, default_value = "8")]
    pub value_len: usize,
    #[clap(long, default_value = "1000000")]
    pub num_ops: u64,
    #[clap(long, default_value = "1")]
    pub get_weight: usize,
    #[clap(long, default_value = "1")]
    pub put_weight: usize,
    #[clap(long, default_value = "1")]
    pub num_threads: usize,
}

struct Bench {
    inner: Arc<Inner>,
}

impl Bench {
    fn new(cfg: Config) -> Self {
        Self {
            inner: Arc::new(Inner::new(cfg)),
        }
    }

    fn run(&self) {
        self.inner.setup();
        for t in 1..=self.inner.cfg.num_threads {
            self.bench(t);
        }
        println!("{:#?}", self.inner.cfg);
        println!("Map {:#?}", self.inner.map.stats());
    }

    fn bench(&self, num_threads: usize) {
        let start = Instant::now();
        let stats = Arc::new(Stats::default());
        let mut threads = Vec::new();
        for _ in 0..num_threads {
            let stats = stats.clone();
            let inner = self.inner.clone();
            threads.push(std::thread::spawn(move || {
                inner.bench(&stats);
            }));
        }
        for thread in threads {
            thread.join().unwrap();
        }
        let elapsed = start.elapsed().as_secs_f64();

        let num_ops = stats.num_ops.get();
        println!(
            "{} threads, {} ops/sec",
            num_threads,
            num_ops as f64 / elapsed
        );
    }
}

struct Inner {
    cfg: Config,
    map: Map,
}

impl Inner {
    fn new(cfg: Config) -> Self {
        Self {
            cfg,
            map: Map::open(Options::default()).unwrap(),
        }
    }

    fn setup(&self) {
        let mut kbuf = vec![0; self.cfg.key_len];
        let mut vbuf = vec![0; self.cfg.value_len];
        let mut workload = Workload::new(&self.cfg);
        // Fills
        for k in 0..self.cfg.num_kvs {
            workload.fill_with_num(k, &mut kbuf);
            workload.fill_with_num(k, &mut vbuf);
            self.map.put(&kbuf, &vbuf).unwrap();
        }
        // Warms up
        for k in 0..(self.cfg.num_ops / 10) {
            workload.fill_with_num(k, &mut kbuf);
            self.map.get(&kbuf, |_| {}).unwrap();
        }
    }

    fn bench(&self, stats: &Stats) {
        let mut kbuf = vec![0; self.cfg.key_len];
        let mut vbuf = vec![0; self.cfg.value_len];
        let mut workload = Workload::new(&self.cfg);
        while stats.num_ops.inc() < self.cfg.num_ops {
            let k = workload.rand_num();
            workload.fill_with_num(k, &mut kbuf);
            match workload.rand_op() {
                Op::Get => {
                    self.map.get(&kbuf, |_| {}).unwrap();
                    stats.num_gets.inc();
                }
                Op::Put => {
                    workload.fill_with_num(k, &mut vbuf);
                    self.map.put(&kbuf, &vbuf).unwrap();
                    stats.num_puts.inc();
                }
            }
        }
    }
}

#[derive(Copy, Clone)]
enum Op {
    Get,
    Put,
}

struct Workload {
    rng: ThreadRng,
    kv_dist: Uniform<u64>,
    op_dist: WeightedIndex<usize>,
    op_choices: [Op; 2],
}

impl Workload {
    fn new(cfg: &Config) -> Self {
        let rng = rand::thread_rng();
        let kv_dist = Uniform::from(0..cfg.num_kvs);
        let op_dist = WeightedIndex::new([cfg.get_weight, cfg.put_weight]).unwrap();
        let op_choices = [Op::Get, Op::Put];
        Self {
            rng,
            kv_dist,
            op_dist,
            op_choices,
        }
    }

    fn rand_op(&mut self) -> Op {
        self.op_choices[self.op_dist.sample(&mut self.rng)]
    }

    fn rand_num(&mut self) -> u64 {
        self.kv_dist.sample(&mut self.rng)
    }

    fn fill_with_num(&mut self, k: u64, buf: &mut [u8]) {
        buf[0..8].copy_from_slice(&k.to_be_bytes());
    }
}

#[derive(Debug, Default)]
struct Stats {
    num_ops: Counter,
    num_gets: Counter,
    num_puts: Counter,
}

#[derive(Debug)]
struct Counter(AtomicU64);

impl Default for Counter {
    fn default() -> Self {
        Self::new(0)
    }
}

impl Counter {
    const fn new(value: u64) -> Self {
        Self(AtomicU64::new(value))
    }

    fn get(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }

    fn inc(&self) -> u64 {
        self.0.fetch_add(1, Ordering::Relaxed)
    }
}

fn main() {
    let cfg = Config::parse();
    let bench = Bench::new(cfg);
    bench.run();
}
