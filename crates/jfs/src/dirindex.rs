//! Directory index tables (volumes with `flag::DIR_INDEX`, made by
//! Linux): every leaf entry carries an index number, and the directory
//! keeps, per number, where the entry is (page and slot) so readdir can
//! resume. Slots 2..12 live in the inode; past that the table moves to
//! pages mapped by a small xtree in the same place.
//!
//! OS/2 volumes have no index tables. rs2 opens volumes with them
//! read-only (see `Volume::mount`), so these are not reached yet.

use crate::inode::Inode;
use crate::{BlockDevice, Error, Result, Volume};

impl<D: BlockDevice> Volume<D> {
    pub(crate) fn index_add(&mut self, _dir: &mut Inode) -> Result<u32> {
        Err(Error::ReadOnly)
    }

    pub(crate) fn index_set(&mut self, _dir: &mut Inode, _index: u32, _page: u64, _slot: usize) -> Result<()> {
        Err(Error::ReadOnly)
    }

    pub(crate) fn index_free(&mut self, _dir: &mut Inode, _index: u32) -> Result<()> {
        Err(Error::ReadOnly)
    }
}
