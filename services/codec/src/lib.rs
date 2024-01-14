#![cfg_attr(target_os = "none", no_std)]

pub mod api;
pub use api::*;
use num_traits::{FromPrimitive, ToPrimitive};
use xous::{send_message, Message, CID};
use xous_ipc::Buffer;

/// This is a keyword reserved for the "arg4" slot of a scalar callback, where args are numbered 1-4.
pub const AUDIO_CB_ROUTING_ID: usize = 0;
#[derive(Debug)]
pub struct Codec {
    conn: CID,
    frame_sid: Option<xous::SID>,
}
impl Codec {
    pub fn new(xns: &xous_names::XousNames) -> Result<Self, xous::Error> {
        REFCOUNT.fetch_add(1, Ordering::Relaxed);
        let conn =
            xns.request_connection_blocking(api::SERVER_NAME_CODEC).expect("Can't connect to Codec server");
        Ok(Codec { conn, frame_sid: None })
    }

    pub fn hook_frame_callback(&mut self, id: u32, cid: CID) -> Result<(), xous::Error> {
        if self.frame_sid.is_none() {
            let sid = xous::create_server().unwrap();
            self.frame_sid = Some(sid);
            let sid_tuple = sid.to_u32();
            xous::create_thread_4(
                frame_cb_server,
                sid_tuple.0 as usize,
                sid_tuple.1 as usize,
                sid_tuple.2 as usize,
                sid_tuple.3 as usize,
            )
            .unwrap();
            let hookdata = ScalarHook { sid: sid_tuple, id, cid };
            let buf = Buffer::into_buf(hookdata).or(Err(xous::Error::InternalError))?;
            buf.lend(self.conn, Opcode::AudioStreamSubscribe.to_u32().unwrap()).map(|_| ())
        } else {
            Err(xous::Error::MemoryInUse) // can't hook it twice
        }
    }

    pub fn setup_8k_stream(&mut self) -> Result<(), xous::Error> {
        send_message(self.conn, Message::new_scalar(Opcode::Setup8kStereo.to_usize().unwrap(), 0, 0, 0, 0))
            .map(|_| ())
    }

    pub fn power_off(&mut self) -> Result<(), xous::Error> {
        send_message(self.conn, Message::new_scalar(Opcode::PowerOff.to_usize().unwrap(), 0, 0, 0, 0))
            .map(|_| ())
    }

    pub fn free_frames(&mut self) -> Result<(usize, usize), xous::Error> {
        let response = send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::FreeFrames.to_usize().unwrap(), 0, 0, 0, 0),
        )?;
        if let xous::Result::Scalar2(play_free, rec_avail) = response {
            Ok((play_free, rec_avail))
        } else {
            log::error!("unexpected return value: {:#?}", response);
            Err(xous::Error::InternalError)
        }
    }

    pub fn swap_frames(&mut self, frames: &mut FrameRing) -> Result<(), xous::Error> {
        let mut buf = Buffer::into_buf(*frames).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, Opcode::SwapFrames.to_u32().unwrap()).or(Err(xous::Error::InternalError))?;

        *frames = buf.to_original::<FrameRing, _>().unwrap();
        Ok(())
    }

    pub fn resume(&mut self) -> Result<(), xous::Error> {
        send_message(self.conn, Message::new_scalar(Opcode::ResumeStream.to_usize().unwrap(), 0, 0, 0, 0))
            .map(|_| ())
    }

    pub fn pause(&mut self) -> Result<(), xous::Error> {
        send_message(self.conn, Message::new_scalar(Opcode::PauseStream.to_usize().unwrap(), 0, 0, 0, 0))
            .map(|_| ())
    }

    pub fn abort(&mut self) -> Result<(), xous::Error> {
        send_message(self.conn, Message::new_scalar(Opcode::AbortStream.to_usize().unwrap(), 0, 0, 0, 0))
            .map(|_| ())
    }

    /// gain goes from 0dB down to -80dB
    pub fn set_speaker_volume(&self, op: VolumeOps, gain: Option<f32>) -> Result<(), xous::Error> {
        let code = if let Some(g) = gain {
            if g > 0.0 { 0 as usize } else { (-g * 10.0) as usize }
        } else {
            800 // -80.0 dB => mute
        };
        send_message(
            self.conn,
            Message::new_scalar(
                Opcode::SetSpeakerVolume.to_usize().unwrap(),
                op.to_usize().unwrap(),
                code, // gain as -dB * 10 as usize
                0,
                0,
            ),
        )
        .map(|_| ())
    }

    /// gain goes from 0dB down to -80dB
    pub fn set_headphone_volume(&self, op: VolumeOps, gain: Option<f32>) -> Result<(), xous::Error> {
        let code = if let Some(g) = gain {
            if g > 0.0 { 0 as usize } else { (-g * 10.0) as usize }
        } else {
            800 // -80.0 dB => mute
        };
        send_message(
            self.conn,
            Message::new_scalar(
                Opcode::SetHeadphoneVolume.to_usize().unwrap(),
                op.to_usize().unwrap(),
                code, // gain as -dB * 10 as usize
                0,
                0,
            ),
        )
        .map(|_| ())
    }

    pub fn is_running(&self) -> Result<bool, xous::Error> {
        match send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::IsLive.to_usize().unwrap(), 0, 0, 0, 0),
        ) {
            Ok(xous::Result::Scalar1(is_live)) => {
                if is_live != 0 {
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            _ => Err(xous::Error::InternalError),
        }
    }

    pub fn poll_headphone_state(&self) -> Result<HeadphoneState, xous::Error> {
        match send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::GetHeadphoneCode.to_usize().unwrap(), 0, 0, 0, 0),
        ) {
            Ok(xous::Result::Scalar1(code)) => {
                let retcode: Option<HeadphoneState> = FromPrimitive::from_usize(code);
                match retcode {
                    Some(code) => Ok(code),
                    None => Err(xous::Error::InternalError),
                }
            }
            _ => Err(xous::Error::InternalError),
        }
    }
}

use core::sync::atomic::{AtomicU32, Ordering};
static REFCOUNT: AtomicU32 = AtomicU32::new(0);
impl Drop for Codec {
    fn drop(&mut self) {
        // de-allocate myself. It's unsafe because we are responsible to make sure nobody else is using the
        // connection.
        if REFCOUNT.fetch_sub(1, Ordering::Relaxed) == 1 {
            unsafe {
                xous::disconnect(self.conn).unwrap();
            }
        }
    }
}

/// handles callback messages that indicate an audio frame has been used up, in the library user's process
/// space.
fn frame_cb_server(sid0: usize, sid1: usize, sid2: usize, sid3: usize) {
    let sid = xous::SID::from_u32(sid0 as u32, sid1 as u32, sid2 as u32, sid3 as u32);
    loop {
        let msg = xous::receive_message(sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(EventCallback::Event) => xous::msg_scalar_unpack!(msg, cid, id, free_play, avail_rec, {
                // directly pass the scalar message onto the CID with the ID memorized in the original hook
                send_message(
                    cid as u32,
                    Message::new_scalar(id, free_play, avail_rec, 0, AUDIO_CB_ROUTING_ID),
                )
                .unwrap();
            }),
            Some(EventCallback::Drop) => {
                break; // this exits the loop and kills the thread
            }
            None => (),
        }
    }
    xous::destroy_server(sid).unwrap();
}
