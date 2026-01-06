use std::{
    fs::File,
    io::{BufRead as _, BufReader},
    path::Path,
};

use anyhow::{Context, Result, anyhow, bail};
use num_traits::Num;

use crate::trace::{Data, MemWrite, RetireEvent, XRegWrite};

pub fn read_trace<Usize: Num>(file_path: &Path) -> Result<Vec<RetireEvent<Usize>>> {
    todo!()
}
