use std::io::{self, Write, Cursor};
use std::hash::{Hash, Hasher};
use std::ops::Deref;
use std::collections::hash_map::RandomState;

use failure::Error;
use indexmap::IndexMap;
use curl::easy::Easy;

#[derive(Clone, Debug)]
pub struct LruCache<K, V, S = RandomState> {
    capacity: usize,
    map: IndexMap<K, V, S>
}
impl<K: Eq + Hash, V> LruCache<K, V> {
    #[inline]
    pub fn new(capacity: usize) -> LruCache<K, V> {
        LruCache { capacity, map: IndexMap::with_capacity(capacity) }
    }
}
impl<K, V, S> Deref for LruCache<K, V> {
    type Target = IndexMap<K, V, S>;

    #[inline(always)]
    fn deref(&self) -> &IndexMap<K, V> {
        &self.map
    }
}


#[inline]
pub fn download_buffer(url: &str) -> Result<Vec<u8>, Error> {
    let mut buffer = Vec::with_capacity(2048);
    {
        let mut cursor = Cursor::new(buffer);
        download(url, &mut cursor)?;
        buffer = cursor.into_inner();
    }
    Ok(buffer)
}


fn download<W: Write>(url: &str, output: &mut W) -> Result<(), Error> {
    let mut easy = Easy::new();
    easy.url(url)?;
    let mut error: Option<io::Error> = None;
    let result = {
        let mut transfer = easy.transfer();
        transfer.write_function(
            |data| if let Err(e) = output.write_all(data) {
                error = Some(e);
                Ok(0)
            } else {
                Ok(data.len())
            },
        )?;
        transfer.perform()
    };
    match result {
        Err(e) => {
            if let Some(actual_error) = error.take() {
                Err(actual_error.into())
            } else {
                Err(e.into())
            }
        }
        Ok(_) => {
            assert!(error.is_none());
            Ok(())
        }
    }
}