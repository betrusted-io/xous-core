mod senres;
mod utils;

use crate::backend::BasisCache;
use crate::backend::PddbOs;
use crate::FileHandle;

use senres::{Senres, SenresMut};

#[repr(u8)]
enum FileType {
    Basis = 0,
    Dict = 1,
    Key = 2,
    DictKey = 3,
    None = 4,
    // Unknown = 5, // Currently unused
}

fn get_fd(
    fds: &mut Vec<Option<crate::FileHandle>>,
    fd: usize,
) -> Result<&mut FileHandle, crate::PddbRetcode> {
    let fd = fds
        .get_mut(fd)
        .ok_or_else(|| {
            log::info!("file handle {} is out of range", fd);
            crate::PddbRetcode::UnexpectedEof
        })?
        .as_mut()
        .ok_or_else(|| {
            log::info!("file handle {} was closed already", fd);
            crate::PddbRetcode::UnexpectedEof
        })?;
    if fd.deleted {
        return Err(crate::PddbRetcode::BasisLost);
    }
    Ok(fd)
}

pub(crate) fn stat_path(
    mem: &mut xous::MemoryMessage,
    pddb_os: &mut PddbOs,
    basis_cache: &mut BasisCache,
) -> Result<(), crate::PddbRetcode> {
    // Convert the memory to a Senres buffer
    let mut backing = senres::Message::from_mut_slice(mem.buf.as_slice_mut())
        .or(Err(crate::PddbRetcode::InternalError))?;

    let reader = backing
        .reader(*b"StaQ")
        .ok_or(crate::PddbRetcode::InternalError)?;
    let path = reader
        .try_get_ref_from::<str>()
        .or(Err(crate::PddbRetcode::InternalError))?
        .to_owned();

    // TODO: use the internal cache inside `basis_cache` to avoid cloning
    let (basis, remainder) =
        utils::split_basis_and_dict(&path, || basis_cache.basis_latest().map(|m| m.to_owned()))
            .or(Err(crate::PddbRetcode::InternalError))?;
    core::mem::drop(reader);

    let mut writer = backing
        .writer(*b"StaR")
        .ok_or(crate::PddbRetcode::InternalError)?;

    // The only time these two are empty is when we want to list the default basis, which
    // itself is not a valid path
    if basis.is_none() && remainder.is_none() {
        writer.append(4u8); // None
        log::error!(
            "path {} has no basis and no remainder, and therefore does not exist",
            path
        );
        return Ok(());
    }

    if let Some(ref basis) = basis {
        if remainder.is_none() {
            let basis_list = basis_cache.basis_list();

            // If the basis matches our name, indicate this is a valid basis
            for s in &basis_list {
                if s == basis {
                    // Kind
                    writer.append(FileType::Basis as u8);
                    // Length
                    writer.append(0u64);
                    return Ok(());
                }
            }

            // Otherwise, indicate it's nothing
            log::error!("remainder is empty on {} and basis doesn't exist", path);
            writer.append(FileType::None as u8);
            return Ok(());
        }
    }

    let stripped_path = remainder.as_deref().unwrap_or("");

    // The root is a dict
    if stripped_path == "" {
        writer.append(FileType::Dict as u8); // Dict
        return Ok(());
    }

    // Find all dicts that match this string
    let dict_list = basis_cache.dict_list(pddb_os, basis.as_deref());
    let is_dict = dict_list.contains(stripped_path);
    let mut is_key = false;

    // Find all keys that are in this dict. Ignore errors, since sometimes
    // the dict doesn't exist, which is fine.
    if let Some((dict_path, key_path)) = stripped_path.rsplit_once(std::path::MAIN_SEPARATOR) {
        if let Some((key_list, _, _)) = basis_cache
            .key_list(pddb_os, dict_path, basis.as_deref())
            .map_err(|e| {
                // log::error!("unable to get key list: {:?}", e);
                e
            })
            .ok()
        {
            if key_list.contains(key_path) {
                is_key = true;
            }
        }
    }

    // Add the count of entries
    let val = match (is_dict, is_key) {
        (true, false) => FileType::Dict,
        (false, true) => FileType::Key,
        (true, true) => FileType::DictKey,
        (false, false) => FileType::None,
    };
    writer.append(val as u8);
    // Placeholder for file length
    writer.append(0u64);

    Ok(())
}

pub(crate) fn list_path(
    mem: &mut xous::MemoryMessage,
    pddb_os: &mut PddbOs,
    basis_cache: &mut BasisCache,
) -> Result<(), crate::PddbRetcode> {
    // Convert the memory to a Senres buffer
    let mut backing = senres::Message::from_mut_slice(mem.buf.as_slice_mut())
        .or(Err(crate::PddbRetcode::InternalError))?;

    let reader = backing
        .reader(*b"PthQ")
        .ok_or(crate::PddbRetcode::InternalError)?;

    let path = reader
        .try_get_ref_from()
        .or(Err(crate::PddbRetcode::InternalError))?;
    let (basis, dict) =
        utils::split_basis_and_dict(path, || basis_cache.basis_latest().map(|m| m.to_owned()))
            .or(Err(crate::PddbRetcode::InternalError))?;

    core::mem::drop(reader);

    let mut writer = backing
        .writer(*b"PthR")
        .ok_or(crate::PddbRetcode::InternalError)?;

    // The only time these two are empty is when we want to list the default basis.
    if basis.is_none() && dict.is_none() {
        let basis_list = basis_cache.basis_list();

        // Add the count of bases
        writer.append(basis_list.len() as u32);

        // Add a list of all bases
        for s in &basis_list {
            // Entry name
            writer.append(s.as_str());
            // Entry kind
            writer.append(0u8);
        }

        return Ok(());
    }

    let dict = dict.as_deref().unwrap_or("");

    // Keep a space at the start of the list for us to log the number of elements.
    let entry_len_pos = writer.delayed_append();

    // Find all dicts that match this string
    let dict_list = basis_cache.dict_list(pddb_os, basis.as_deref());
    // Find all keys that are in this dict. Ignore errors, since sometimes
    // the dict doesn't exist, which is fine.
    let (key_list, _, _) = basis_cache
        .key_list(pddb_os, dict, basis.as_deref())
        .map_err(|e| {
            // log::error!("unable to get key list: {:?}", e);
            e
        })
        .unwrap_or_default();

    let mut entries_count = 0u32;

    for key in key_list.iter() {
        // If the entry exists, turn it into a DictKey
        let kind = if dict_list.contains(key) { 3u8 } else { 2u8 };
        // Entry name
        writer.append(key.as_str());
        // Entry kind
        writer.append(kind);
        entries_count += 1;
    }

    for dict in dict_list
        .iter()
        .filter(|needle| utils::get_path(&needle, dict).is_some())
        .filter(|needle| !key_list.contains(needle.as_str()))
    {
        if let Some((_, end)) = dict.rsplit_once(':') {
            // Entry name
            writer.append(end);
            // Entry kind
            writer.append(1u8);
            entries_count += 1;
        }
    }

    // Add the count of entries
    writer.do_delayed_append(entry_len_pos, entries_count);

    Ok(())
}

pub(crate) fn list_basis(
    mem: &mut xous::MemoryMessage,
    basis_cache: &mut BasisCache,
) -> Result<(), crate::PddbRetcode> {
    let basis_list = basis_cache.basis_list();

    // Convert the memory to a Senres buffer
    let mut backing = senres::Message::from_mut_slice(mem.buf.as_slice_mut())
        .or(Err(crate::PddbRetcode::InternalError))?;

    {
        backing
            .reader(*b"basQ")
            .ok_or(crate::PddbRetcode::InternalError)?;
    }
    let mut writer = backing
        .writer(*b"basR")
        .ok_or(crate::PddbRetcode::InternalError)?;

    // Add the count of bases
    writer.append(basis_list.len() as u32);

    // Add a list of all bases
    for s in &basis_list {
        writer.append(s.as_str());
    }

    Ok(())
}

/// Also called `KeyRequest` for non-libstd calls
pub(crate) fn open_key(
    mem: &mut xous::MemoryMessage,
    pddb_os: &mut PddbOs,
    basis_cache: &mut BasisCache,
    fds: &mut Vec<Option<crate::FileHandle>>,
) -> Result<(), crate::PddbRetcode> {
    // Convert the memory to a Senres buffer
    let mut backing = senres::Message::from_mut_slice(mem.buf.as_slice_mut())
        .or(Err(crate::PddbRetcode::InternalError))?;

    let create_file: bool;
    let create_path: bool;
    let create_new: bool;
    let append: bool;
    let truncate: bool;
    let alloc_hint: usize;
    let cb_sid: Option<xous::SID>;
    let (requested_basis, requested_dict) = {
        let reader = backing
            .reader(*b"KyOQ")
            .ok_or(crate::PddbRetcode::InternalError)?;

        let path = reader
            .try_get_ref_from()
            .or(Err(crate::PddbRetcode::InternalError))?;
        create_file = reader
            .try_get_from()
            .or(Err(crate::PddbRetcode::InternalError))?;

        create_path = reader
            .try_get_from()
            .or(Err(crate::PddbRetcode::InternalError))?;

        create_new = reader
            .try_get_from()
            .or(Err(crate::PddbRetcode::InternalError))?;

        append = reader
            .try_get_from()
            .or(Err(crate::PddbRetcode::InternalError))?;

        truncate = reader
            .try_get_from()
            .or(Err(crate::PddbRetcode::InternalError))?;

        alloc_hint = reader
            .try_get_from::<u64>()
            .or(Err(crate::PddbRetcode::InternalError))? as usize;

        cb_sid = if let Some(sid) = reader
            .try_get_from()
            .or(Err(crate::PddbRetcode::InternalError))?
        {
            Some(xous::SID::from_array(sid))
        } else {
            None
        };
        utils::split_basis_and_dict(path, || basis_cache.basis_latest().map(|m| m.to_owned()))
            .or(Err(crate::PddbRetcode::BasisLost))?
    };

    // Ensure the user passed a valid dict
    let requested_dict = requested_dict.ok_or_else(|| {
        log::error!("no dict was specified");
        crate::PddbRetcode::BasisLost
    })?;

    // Split the final ":key" off from the dict. If there are no ":" then this will fail
    // with the "no key was specified" error.
    let (requested_dict, requested_key) = requested_dict
        .rsplit_once(std::path::MAIN_SEPARATOR)
        .ok_or_else(|| {
            log::error!("no key was specified");
            crate::PddbRetcode::AccessDenied
        })?;

    let mut writer = backing
        .writer(*b"KyOR")
        .ok_or(crate::PddbRetcode::InternalError)?;

    // Behavior for opening files:
    // 1. If `bname` is defined, then we only loop once.
    for basis in basis_cache.access_list().iter() {
        if requested_basis.is_some() && Some(basis.as_str()) != requested_basis.as_deref() {
            continue;
        }
        let bname = Some(basis.as_str());

        // If the dictionary wasn't found, either add it or move on
        if basis_cache
            .dict_attributes(pddb_os, requested_dict, bname)
            .is_err()
        {
            if !create_path {
                continue;
            }
            basis_cache
                .dict_add(pddb_os, requested_dict, bname)
                .map_err(|e| {
                    log::error!(
                        "unable to add dict {} to basis {}: {:?}",
                        requested_dict,
                        bname.unwrap_or("internal_error"),
                        e
                    );
                    crate::PddbRetcode::InternalError
                })?;
        }

        let mut len = 0;

        // If the file doesn't exist in this basis, create it (if necessary)
        // or mvoe on to the next basis.
        if basis_cache
            .key_attributes(pddb_os, requested_dict, requested_key, bname)
            .map(|attr| len = attr.len as u64)
            .is_err()
        {
            if !create_file {
                continue;
            }
            // create an empty key placeholder
            basis_cache
                .key_update(
                    pddb_os,
                    requested_dict,
                    requested_key,
                    &[],
                    None,
                    if alloc_hint > 0 {
                        Some(alloc_hint)
                    } else {
                        None
                    },
                    bname,
                    true,
                )
                .map_err(|e| {
                    log::error!(
                        "unable to add dict {} to basis {}: {:?}",
                        requested_dict,
                        bname.unwrap_or("internal_error"),
                        e,
                    );
                    crate::PddbRetcode::InternalError
                })?;
            len = 0;
        } else if create_new {
            log::error!(
                "user requested to create {}{}{} with `create_new` set, but that file already exists",
                requested_dict, std::path::MAIN_SEPARATOR, requested_key
            );
            return Err(crate::PddbRetcode::DiskFull);
        } else if truncate {
            // Truncate the file, which we know exists
            basis_cache
                .key_update(
                    pddb_os,
                    requested_dict,
                    requested_key,
                    &[],
                    None,
                    if alloc_hint > 0 {
                        Some(alloc_hint)
                    } else {
                        None
                    },
                    bname,
                    true,
                )
                .map_err(|e| {
                    log::error!(
                        "unable to add dict {} to basis {}: {:?}",
                        requested_dict,
                        bname.unwrap_or("internal_error"),
                        e,
                    );
                    crate::PddbRetcode::InternalError
                })?;
        }

        // The basis exists for sure.
        let conn =
            cb_sid.map(|cb_sid| xous::connect(cb_sid).expect("couldn't connect for callback"));
        let file_handle = crate::FileHandle {
            dict: String::from(requested_dict),
            key: String::from(requested_key),
            basis: bname.map(|s| s.to_owned()),
            offset: if append { len } else { 0 },
            length: len,
            deleted: false,
            conn,
            alloc_hint: if alloc_hint > 0 {
                Some(alloc_hint)
            } else {
                None
            },
        };

        // Look for the first `None` record in the file descriptor list. If there
        // isn't one, push it to the end.
        let mut fd = None;
        let mut token_record = Some(file_handle);
        for (index, item) in fds.iter_mut().enumerate() {
            if item.is_none() {
                *item = token_record.take();
                fd = Some(index);
                break;
            }
        }
        let fd = fd.unwrap_or_else(|| {
            fds.push(token_record.take());
            fds.len() - 1
        });

        // Add the file descriptor number
        writer.append(fd as u16);

        // Add the file length
        writer.append(len);

        return Ok(());
    }

    log::error!(
        "unable to find key {} in dict {}",
        requested_key,
        requested_dict
    );

    // If we fall off the end, then the dict/key do not exist
    Err(crate::PddbRetcode::BasisLost)
}

pub(crate) fn close_key(
    fds: &mut Vec<Option<crate::FileHandle>>,
    fd: usize,
) -> Result<(), crate::PddbRetcode> {
    let file = fds.get_mut(fd).ok_or_else(|| {
        log::info!("file handle {} is out of range", fd);
        crate::PddbRetcode::UnexpectedEof
    })?;

    // Remove the file from the list, replacing it with None
    let conn = match file.take() {
        Some(s) => s.conn,
        None => {
            log::info!("file handle {} was closed already", fd);
            return Err(crate::PddbRetcode::UnexpectedEof);
        }
    };

    // If this SID is unused, close the connection.
    if let Some(cid) = conn {
        let mut found_duplicate = false;
        for entry in fds.iter() {
            if let Some(other_fd) = entry {
                if other_fd.conn == conn {
                    found_duplicate = true;
                    break;
                }
            }
        }
        if !found_duplicate {
            unsafe { xous::disconnect(cid).or(Err(crate::PddbRetcode::InternalError))? };
        }
    }

    // Shorten the list if we just removed the final element
    if fd == fds.len() {
        fds.pop();
    }

    Ok(())
}

pub(crate) fn delete_key(
    mem: &mut xous::MemoryMessage,
    pddb_os: &mut PddbOs,
    basis_cache: &mut BasisCache,
    all_fds: &mut std::collections::HashMap<Option<xous::PID>, Vec<Option<FileHandle>>>,
) -> Result<(), crate::PddbRetcode> {
    // Convert the memory to a Senres buffer
    let backing = senres::Message::from_mut_slice(mem.buf.as_slice_mut())
        .or(Err(crate::PddbRetcode::InternalError))?;

    let reader = backing
        .reader(*b"RmKQ")
        .ok_or(crate::PddbRetcode::InternalError)?;

    let full_path = reader
        .try_get_ref_from::<str>()
        .or(Err(crate::PddbRetcode::BasisLost))?;

    let (basis, path) = utils::split_basis_and_dict(full_path, || {
        basis_cache.basis_latest().map(|m| m.to_owned())
    })
    .or(Err(crate::PddbRetcode::AccessDenied))?;
    let path = path.ok_or(crate::PddbRetcode::AccessDenied)?;
    let bname = basis.as_deref();
    let (dict, key) = path
        .rsplit_once(std::path::MAIN_SEPARATOR)
        .ok_or(crate::PddbRetcode::AccessDenied)?;

    // Perform the actual removal
    basis_cache
        .key_remove(pddb_os, dict, key, bname, false)
        .or_else(|e| {
            log::error!(
                "unable to delete key {} in dict {} (basis {:?}): {:?}",
                key,
                dict,
                bname,
                e
            );
            Err(crate::PddbRetcode::UnexpectedEof)
        })?;

    // Mark the entry as deleted in all remaining file handles in the entire system
    for fds in all_fds.values_mut() {
        for fd in fds
            .iter_mut()
            .filter(|f| f.is_some())
            .map(|f| f.as_mut().unwrap())
        {
            if fd.basis == basis && fd.key == key && fd.dict == dict {
                fd.deleted = true;
            }
        }
    }

    Ok(())
}

pub(crate) fn write_key(
    mem: &mut xous::MemoryMessage,
    pddb_os: &mut PddbOs,
    basis_cache: &mut BasisCache,
    fds: &mut Vec<Option<crate::FileHandle>>,
    fd: usize,
) -> Result<(), crate::PddbRetcode> {
    let file = get_fd(fds, fd)?;
    let mut retcode = crate::PddbRetcode::InternalError;

    for basis in basis_cache.access_list().iter() {
        log::debug!(
            "write (spec: {:?}){:?} {}",
            file.basis,
            file.basis.as_ref().unwrap_or(basis),
            file.key
        );
        let length_to_write = mem.valid.map(|v| v.get()).unwrap_or_default();
        if basis_cache
            .key_update(
                pddb_os,
                &file.dict,
                &file.key,
                &mem.buf.as_slice_mut()[0..length_to_write],
                // &mut pbuf.data[..pbuf.len as usize],
                Some(file.offset as usize),
                file.alloc_hint,
                // this is a bit inefficient because if a specific basis is specified *and* the key does not exist,
                // it'll retry the same basis for a number of times equal to the number of bases open.
                // However, usually, there's only 1-2 bases open, and usually, if you specify a basis,
                // the key will be a hit, so, we let it stand.
                Some(file.basis.as_ref().unwrap_or(basis)),
                false,
            )
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => retcode = crate::PddbRetcode::BasisLost,
                std::io::ErrorKind::UnexpectedEof => retcode = crate::PddbRetcode::UnexpectedEof,
                std::io::ErrorKind::OutOfMemory => retcode = crate::PddbRetcode::DiskFull,
                _ => retcode = crate::PddbRetcode::InternalError,
            })
            .is_ok()
        {
            file.offset += length_to_write as u64;
            mem.valid = xous::MemorySize::new(length_to_write);
            return Ok(());
        }
    }

    Err(retcode)
}

pub(crate) fn seek_key(
    seek_type: usize,
    by_offset: u64,
    fds: &mut Vec<Option<crate::FileHandle>>,
    fd: usize,
) -> Result<u64, crate::PddbRetcode> {
    let file = get_fd(fds, fd)?;

    fn seek_from_point(
        this: &mut FileHandle,
        point: u64,
        by: i64,
    ) -> Result<u64, crate::PddbRetcode> {
        let by64 = by as u64;
        // Note that it's possible to seek past the end of a key, and in this case
        // the `offset` will be greater than the `len`. This is fine, and `len` will
        // be updated as soon as `write()` is called.
        if by < 0 {
            this.offset = point.checked_sub(by64).ok_or_else(|| {
                // std::io::Error::new(std::io::ErrorKind::InvalidInput, "cannot seek before 0")
                log::error!("cannot seek before 0");
                crate::PddbRetcode::UnexpectedEof
            })?;
        } else {
            this.offset = point.checked_add(by64).ok_or_else(|| {
                // std::io::Error::new(std::io::ErrorKind::InvalidInput, "seek overflowed")
                log::error!("seek overflowed");
                crate::PddbRetcode::UnexpectedEof
            })?;
        }
        Ok(this.offset)
    }

    match seek_type {
        // SeekFrom::Start(offset)
        0 => seek_from_point(file, 0, by_offset as i64),
        // SeekFrom::Current(by)
        1 => seek_from_point(file, file.offset, by_offset as i64),
        // SeekFrom::End(by)
        2 => seek_from_point(file, file.length, by_offset as i64),
        _ => Err(crate::PddbRetcode::UnexpectedEof),
    }
}

pub(crate) fn read_key(
    mem: &mut xous::MemoryMessage,
    pddb_os: &mut PddbOs,
    basis_cache: &mut BasisCache,
    fds: &mut Vec<Option<crate::FileHandle>>,
    fd: usize,
) -> Result<(), crate::PddbRetcode> {
    let file = get_fd(fds, fd)?;

    let mut retcode = crate::PddbRetcode::Ok;
    for basis in basis_cache.access_list().iter() {
        log::debug!(
            "read (spec: {:?}){:?} {}",
            file.basis,
            file.basis.as_ref().unwrap_or(basis),
            file.key
        );
        if let Ok(readlen) = basis_cache
            .key_read(
                pddb_os,
                &file.dict,
                &file.key,
                &mut mem.buf.as_slice_mut()[0..mem.valid.map(|v| v.get()).unwrap_or_default()],
                Some(file.offset as usize),
                // this is a bit inefficient because if a specific basis is specified *and* the key does not exist,
                // it'll retry the same basis for a number of times equal to the number of bases open.
                // However, usually, there's only 1-2 bases open, and usually, if you specify a basis,
                // the key will be a hit, so, we let it stand.
                Some(file.basis.as_ref().unwrap_or(basis)),
            )
            .map_err(|e| {
                match e.kind() {
                    std::io::ErrorKind::NotFound => retcode = crate::PddbRetcode::BasisLost,
                    std::io::ErrorKind::UnexpectedEof => {
                        retcode = crate::PddbRetcode::UnexpectedEof
                    }
                    std::io::ErrorKind::OutOfMemory => retcode = crate::PddbRetcode::DiskFull,
                    _ => retcode = crate::PddbRetcode::InternalError,
                };
            })
        {
            file.offset += readlen as u64;
            mem.valid = xous::MemorySize::new(readlen);

            return Ok(());
        }
    }
    log::error!(
        "couldn't find file {} in dict {} in basis {:?}",
        file.key,
        file.dict,
        file.basis
    );
    Err(retcode)
}

pub(crate) fn list_dict(
    mem: &mut xous::MemoryMessage,
    pddb_os: &mut PddbOs,
    basis_cache: &mut BasisCache,
) -> Result<(), crate::PddbRetcode> {
    let mut backing = senres::Message::from_mut_slice(mem.buf.as_slice_mut())
        .or(Err(crate::PddbRetcode::InternalError))?;
    let bname;
    {
        let reader = backing
            .reader(*b"LiDQ")
            .ok_or(crate::PddbRetcode::InternalError)?;
        bname = reader
            .try_get_from::<Option<String>>()
            .map_err(|_| crate::PddbRetcode::InternalError)?;
    }

    let mut writer = backing
        .writer(*b"LiDR")
        .ok_or(crate::PddbRetcode::InternalError)?;

    let dict_list = basis_cache.dict_list(pddb_os, bname.as_deref());
    writer.append(dict_list.len() as u32);
    for dict in dict_list.iter() {
        writer.append(dict.as_str());
    }

    Ok(())
}

pub(crate) fn list_key(
    mem: &mut xous::MemoryMessage,
    pddb_os: &mut PddbOs,
    basis_cache: &mut BasisCache,
) -> Result<(), crate::PddbRetcode> {
    let mut backing = senres::Message::from_mut_slice(mem.buf.as_slice_mut())
        .or(Err(crate::PddbRetcode::InternalError))?;
    let bname;
    let key;
    {
        let reader = backing
            .reader(*b"LiKQ")
            .ok_or(crate::PddbRetcode::InternalError)?;
        bname = reader
            .try_get_from::<Option<String>>()
            .map_err(|_| crate::PddbRetcode::InternalError)?;
        key = reader
            .try_get_ref_from::<str>()
            .map_err(|_| crate::PddbRetcode::InternalError)?
            .to_owned();
    }

    let mut writer = backing
        .writer(*b"LiKR")
        .ok_or(crate::PddbRetcode::InternalError)?;

    let (key_list, _, _) = basis_cache
        .key_list(pddb_os, &key, bname.as_deref())
        .or_else(|e| {
            log::error!(
                "unable to get key list of dict {} in basis {:?}: {:?}",
                key,
                bname,
                e
            );
            Err(crate::PddbRetcode::InternalError)
        })?;

    writer.append(key_list.len() as u32);
    for dict in key_list.iter() {
        writer.append(dict.as_str());
    }

    Ok(())
}

pub(crate) fn delete_dict(
    mem: &mut xous::MemoryMessage,
    pddb_os: &mut PddbOs,
    basis_cache: &mut BasisCache,
) -> Result<(), crate::PddbRetcode> {
    let backing = senres::Message::from_mut_slice(mem.buf.as_slice_mut())
        .or(Err(crate::PddbRetcode::InternalError))?;
    let reader = backing
        .reader(*b"RmDQ")
        .ok_or(crate::PddbRetcode::InternalError)?;

    let path = reader
        .try_get_ref_from::<str>()
        .or(Err(crate::PddbRetcode::InternalError))?;
    let (bname, dict) =
        utils::split_basis_and_dict(path, || basis_cache.basis_latest().map(|m| m.to_owned()))
            .or(Err(crate::PddbRetcode::InternalError))?;
    let dict = dict.ok_or(crate::PddbRetcode::InternalError)?;

    if let Some((key_list, _, _)) = basis_cache
        .key_list(pddb_os, &dict, bname.as_deref())
        .map_err(|e| {
            // log::error!("unable to get key list: {:?}", e);
            e
        })
        .ok()
    {
        if !key_list.is_empty() {
            log::error!("directory {} is not empty", dict);
            return Err(crate::PddbRetcode::DiskFull);
        }
    }

    if basis_cache
        .dict_remove(pddb_os, &dict, bname.as_deref(), false)
        .is_err()
    {
        log::error!("error removing dict {} in basis {:?}", dict, bname);
        return Err(crate::PddbRetcode::InternalError);
    }

    Ok(())
}

pub(crate) fn create_dict(
    mem: &mut xous::MemoryMessage,
    pddb_os: &mut PddbOs,
    basis_cache: &mut BasisCache,
) -> Result<(), crate::PddbRetcode> {
    let backing = senres::Message::from_mut_slice(mem.buf.as_slice_mut())
        .or(Err(crate::PddbRetcode::InternalError))?;
    let reader = backing
        .reader(*b"NuDQ")
        .ok_or(crate::PddbRetcode::InternalError)?;

    let path = reader
        .try_get_ref_from::<str>()
        .or(Err(crate::PddbRetcode::InternalError))?;
    let (basis, dict) =
        utils::split_basis_and_dict(path, || basis_cache.basis_latest().map(|m| m.to_owned()))
            .or(Err(crate::PddbRetcode::InternalError))?;
    let dict = dict.ok_or(crate::PddbRetcode::InternalError)?;

    basis_cache
        .dict_add(pddb_os, &dict, basis.as_deref())
        .map_err(|e| {
            log::error!(
                "unable to add dict {} to basis {}: {:?}",
                dict,
                basis.as_deref().unwrap_or("internal_error"),
                e
            );
            crate::PddbRetcode::InternalError
        })?;

    Ok(())
}
