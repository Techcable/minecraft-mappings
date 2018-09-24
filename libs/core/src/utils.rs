use std::io::{self, Write, Cursor};
use std::hash::{Hash, BuildHasher};
use std::ops::Deref;
use std::collections::hash_map::RandomState;
use std::path::Path;
use std::str;

use failure::Error;
use failure_derive::Fail;
use indexmap::{IndexMap, map::Entry};
use git2::{Repository, Commit};
use curl::easy::Easy;

#[derive(Clone, Debug)]
pub struct LruCache<K: Eq + Hash, V, S: BuildHasher = RandomState> {
    capacity: usize,
    map: IndexMap<K, V, S>
}
impl<K: Eq + Hash, V> LruCache<K, V> {
    #[inline]
    pub fn new(capacity: usize) -> LruCache<K, V> {
        LruCache { capacity, map: IndexMap::with_capacity(capacity) }
    }
    fn cleanup(&mut self) {
        assert!(self.map.len() >= self.capacity);
        let needed_removed = self.map.len() - self.capacity;
        let mut index = 0;
        self.map.retain(|_, _| {
            let should_remove = index < needed_removed;
            index += 1;
            should_remove
        });
        assert!(self.map.len() <= self.capacity);
    }
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        let (old, cleanup) = match self.map.entry(key) {
            Entry::Occupied(mut entry) => (Some(entry.insert(value)), false),
            Entry::Vacant(entry) => {
                entry.insert(value);
                (None, true)
            }
        };
        if cleanup {
            self.cleanup()
        }
        old
    }
}
impl<K: Eq + Hash, V, S: BuildHasher> Deref for LruCache<K, V, S> {
    type Target = IndexMap<K, V, S>;

    #[inline(always)]
    fn deref(&self) -> &IndexMap<K, V, S> {
        &self.map
    }
}

pub fn load_from_commit(repo: &Repository, commit: &Commit, relative_path: &Path, buffer: &mut String) -> Result<(), Error> {
    let tree = commit.tree()?;
    let object = tree.get_path(relative_path)?.to_object(repo)?;
    // TODO: Don't panic
    let blob = object.into_blob().unwrap_or_else(|e| {
        panic!(
            "Expected {} to be a blob, not a {:?}",
            relative_path.display(),
            e.kind()
        )
    });
    buffer.push_str(str::from_utf8(blob.content())?);
    Ok(())
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
    easy.fail_on_error(true)?;
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
    if easy.response_code()? == 404 {
        return Err(HttpNotFound.into())
    }
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
#[derive(Debug, Fail)]
#[fail(display = "HTTP 404 not found")]
pub struct HttpNotFound;