use std::sync::{Arc, LazyLock};

use tokio::{fs::File, io::BufWriter};

use crate::Level;

pub(crate) type FileWriters = Arc<tokio::sync::Mutex<Option<Vec<(Level, BufWriter<File>)>>>>;

pub(crate) static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(reqwest::Client::new);

pub(crate) static RT: LazyLock<tokio::runtime::Runtime> = LazyLock::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .max_blocking_threads(1)
        .build()
        .unwrap()
});
