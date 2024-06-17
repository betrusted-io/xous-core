#![cfg_attr(target_os = "none", no_std)]

pub mod api;
use api::*;
#[cfg(feature = "ditherpunk")]
pub mod tests;

use core::cell::Cell;
#[cfg(feature = "ditherpunk")]
use std::cmp::max;
#[cfg(feature = "ditherpunk")]
use std::convert::TryInto;

use bit_field::BitField;
use gam::*;
use num_traits::*;
use xous::{send_message, Message, CID};
use xous_ipc::Buffer;

pub type TextValidationFn = fn(TextEntryPayload) -> Option<ValidatorErr>;

pub struct AlertModalBuilder<'a> {
    prompt: String,
    validators: Vec<Option<TextValidationFn>>,
    placeholders: Vec<Option<(String, bool)>>,
    growable: bool,
    modals: &'a Modals,
}

impl<'a> AlertModalBuilder<'a> {
    /// Placeholders, when provided, disappear on keypress or backspace; they persist with left/right arrows.
    /// This is useful for suggesting an input, or explaining what a field does.
    pub fn field(
        &'a mut self,
        placeholder: Option<String>,
        validator: Option<TextValidationFn>,
    ) -> &'a mut Self {
        self.validators.push(validator);
        if let Some(p) = placeholder {
            self.placeholders.push(Some((p, false)));
        } else {
            self.placeholders.push(None)
        }
        self
    }

    /// Placeholders provided in this method persist when any keys are pressed, instead of disappearing.
    /// This is useful for "edit" functions.
    pub fn field_placeholder_persist(
        &'a mut self,
        placeholder: Option<String>,
        validator: Option<TextValidationFn>,
    ) -> &'a mut Self {
        self.validators.push(validator);
        if let Some(p) = placeholder {
            self.placeholders.push(Some((p, true)));
        } else {
            self.placeholders.push(None)
        }
        self
    }

    pub fn set_growable(&'a mut self) -> &'a mut Self {
        self.growable = true;
        self
    }

    pub fn build(&self) -> Result<TextEntryPayloads, xous::Error> {
        self.modals.lock();
        let mut final_placeholders: Option<[Option<(xous_ipc::String<256>, bool)>; 10]> = None;
        let fields_amt = self.validators.len();

        if fields_amt == 0 {
            log::error!("must add at least one field to alert");
            self.modals.unlock();
            return Err(xous::Error::UnknownError);
        }

        match self.placeholders.len() {
            1.. => {
                let mut pl: [Option<(xous_ipc::String<256>, bool)>; 10] = Default::default();

                if fields_amt != self.placeholders.len() {
                    log::warn!("can't have more fields than placeholders");
                    self.modals.unlock();
                    return Err(xous::Error::UnknownError);
                }

                for (index, placeholder) in self.placeholders.iter().enumerate() {
                    if let Some((string, persist)) = placeholder {
                        pl[index] = Some((xous_ipc::String::from_str(&string), *persist))
                    } else {
                        pl[index] = None
                    }
                }

                final_placeholders = Some(pl)
            }
            0 => (),
            // Note: if you are getting an error on compilation, this line below is required for Rust < 1.75.0
            // _ => panic!("somehow len of placeholders was neither zero or >= 1...?"),
        }

        let mut spec = ManagedPromptWithTextResponse {
            token: self.modals.token,
            prompt: xous_ipc::String::from_str(&self.prompt),
            fields: fields_amt as u32,
            placeholders: final_placeholders,
            growable: self.growable,
        };

        // question: do we want to add a retry limit?
        loop {
            let mut buf = Buffer::into_buf(spec).or(Err(xous::Error::InternalError))?;
            buf.lend_mut(self.modals.conn, Opcode::PromptWithTextResponse.to_u32().unwrap())
                .or(Err(xous::Error::InternalError))?;
            match buf.to_original::<TextEntryPayloads, _>() {
                Ok(response) => {
                    let mut form_validation_failed = false;
                    for (index, validator) in self.validators.iter().enumerate() {
                        if let Some(validator) = validator {
                            if let Some(err_msg) = validator(response.content()[index]) {
                                spec.prompt.clear();
                                spec.prompt.append(err_msg.as_str().unwrap_or("UTF-8 error")).ok();
                                form_validation_failed = true;
                                break; // one of the validator failed
                            }
                        }
                    }

                    if form_validation_failed {
                        continue; // leave the modal as it is
                    }

                    // If we're here all non-None validators returned okay, or no validators were specified in
                    // the first place at all.
                    send_message(
                        self.modals.conn,
                        Message::new_blocking_scalar(
                            Opcode::TextResponseValid.to_usize().unwrap(),
                            self.modals.token[0] as _,
                            self.modals.token[1] as _,
                            self.modals.token[2] as _,
                            self.modals.token[3] as _,
                        ),
                    )
                    .expect("couldn't acknowledge text entry");
                    self.modals.unlock();
                    return Ok(response);
                }
                _ => {
                    // we send the valid response token even in this case because we want the modals server to
                    // move on and not get stuck on this error.
                    send_message(
                        self.modals.conn,
                        Message::new_blocking_scalar(
                            Opcode::TextResponseValid.to_usize().unwrap(),
                            self.modals.token[0] as _,
                            self.modals.token[1] as _,
                            self.modals.token[2] as _,
                            self.modals.token[3] as _,
                        ),
                    )
                    .expect("couldn't acknowledge text entry");
                    self.modals.unlock();
                    return Err(xous::Error::InternalError);
                }
            }
        }
    }
}

pub struct Modals {
    conn: CID,
    token: [u32; 4],
    have_lock: Cell<bool>,
}
impl Modals {
    pub fn new(xns: &xous_names::XousNames) -> Result<Self, xous::Error> {
        REFCOUNT.fetch_add(1, Ordering::Relaxed);
        let conn =
            xns.request_connection_blocking(api::SERVER_NAME_MODALS).expect("Can't connect to Modals server");
        #[cfg(feature = "cramium-soc")]
        let trng = cram_hal_service::trng::Trng::new(&xns).unwrap();
        #[cfg(not(feature = "cramium-soc"))]
        let trng = trng::Trng::new(&xns).unwrap();
        let mut token = [0u32; 4];
        trng.fill_buf(&mut token).unwrap();
        Ok(Modals { conn, token, have_lock: Cell::new(false) })
    }

    pub fn alert_builder(&self, prompt: &str) -> AlertModalBuilder {
        AlertModalBuilder {
            prompt: String::from(prompt),
            validators: vec![],
            placeholders: vec![],
            modals: self,
            growable: false,
        }
    }

    /// Text/QR code notification modal dialog.
    ///
    /// - `qrtext` turns submitted text into a qr code.
    /// - This dialog blocks until the notification has been acknowledged via [ Press any key ].
    /// - Dialog does not scroll, burden is on the consumer to make sure text + qr code do not overflow
    ///   available space.
    ///
    /// <details>
    ///     <summary>Example Image</summary>
    ///
    /// ![Example Image](https://github.com/betrusted-io/xous-core/blob/main/docs/images/modals_show_notification.png?raw=true)
    ///
    /// </details>
    ///
    /// # Examples
    /// ```
    /// use modals::Modals;
    /// use xous_names::XousNames;
    /// let xns = XousNames::new().unwrap();
    /// let modals = Modals::new(&xns).unwrap();
    /// modals
    ///     .show_notification(
    ///         "Check the wiki:",
    ///         Some("https://github.com/betrusted-io/betrusted-wiki/wiki"),
    ///     )
    ///     .unwrap();
    /// ```
    pub fn show_notification(&self, notification: &str, qrtext: Option<&str>) -> Result<(), xous::Error> {
        self.lock();
        let qrtext = match qrtext {
            Some(text) => Some(xous_ipc::String::from_str(text)),
            None => None,
        };
        let spec = ManagedNotification {
            token: self.token,
            message: xous_ipc::String::from_str(notification),
            qrtext,
        };
        let buf = Buffer::into_buf(spec).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, Opcode::Notification.to_u32().unwrap()).or(Err(xous::Error::InternalError))?;
        self.unlock();
        Ok(())
    }

    /// Modal dialog used to show up to 256 bits of `data` in bip39 format.
    ///
    /// - This dialog blocks until the notification has been acknowledged via [ Press any key ].
    /// - Data must conform to the codeable lengths by BIP39, or else the routine
    /// will return immediately with an `InvalidString` error without showing any dialog box.
    ///
    /// <details>
    ///     <summary>Example Image</summary>
    ///
    /// ![Example Image](https://github.com/rowr111/xous-core/blob/main/docs/images/modals_show_bip39.png?raw=true)
    ///
    /// </details>
    ///
    /// # Example
    /// ```
    /// use modals::Modals;
    /// use xous_names::XousNames;
    /// let xns = XousNames::new().unwrap();
    /// let modals = Modals::new(&xns).unwrap();
    ///
    /// let refnum = 0b00000110001101100111100111001010000110110010100010110101110011111101101010011100000110000110101100110110011111100010011100011110u128;
    /// let refvec = refnum.to_be_bytes().to_vec();
    /// modals.show_bip39(Some("Some bip39 words"), &refvec).expect("couldn't show bip39 words");
    /// ```
    pub fn show_bip39(&self, caption: Option<&str>, data: &Vec<u8>) -> Result<(), xous::Error> {
        match data.len() {
            16 | 20 | 24 | 28 | 32 => (),
            _ => return Err(xous::Error::InvalidString),
        }
        self.lock();
        let mut bip39_data = [0u8; 32];
        for (&s, d) in data.iter().zip(bip39_data.iter_mut()) {
            *d = s;
        }
        let spec = ManagedBip39 {
            token: self.token,
            caption: if let Some(c) = caption { Some(xous_ipc::String::from_str(c)) } else { None },
            bip39_data,
            bip39_len: if data.len() <= 32 { data.len() as u32 } else { 32 },
        };
        let buf = Buffer::into_buf(spec).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, Opcode::Bip39.to_u32().unwrap()).or(Err(xous::Error::InternalError))?;
        self.unlock();
        Ok(())
    }

    /// Input dialog used to accept bip39 formatted data.
    ///
    /// - This dialog blocks until either \[ F4 \] has been pressed to abort entry, or a phrase has been input
    ///   and accepted via \[ enter \].
    /// - Possible words will be auto-suggested during typing and \[ enter \] must be pressed after each word.
    /// - Valid input will be shown in the bottom section of the dialog after an accepted phrase has been
    ///   entered.
    ///
    /// <details>
    ///     <summary>Example Images (from example code below)</summary>
    ///
    /// ![Example Image - Initial](https://github.com/rowr111/xous-core/blob/main/docs/images/modals_input_bip39_1.png?raw=true)
    /// ![Example Image - Input](https://github.com/rowr111/xous-core/blob/main/docs/images/modals_input_bip39_2.png?raw=true)
    /// ![Example Image - Confirmation](https://github.com/rowr111/xous-core/blob/main/docs/images/modals_input_bip39_3.png?raw=true)
    ///
    /// </details>
    ///
    /// # Example
    /// ```
    /// use modals::Modals;
    /// use xous_names::XousNames;
    /// let xns = XousNames::new().unwrap();
    /// let modals = Modals::new(&xns).unwrap();
    ///
    /// log::info!(
    ///     "type these words: alert record income curve mercy tree heavy loan hen recycle mean devote"
    /// );
    /// match modals.input_bip39(Some("Input BIP39 words")) {
    ///     Ok(data) => {
    ///         log::info!("got bip39 input: {:x?}", data);
    ///         log::info!("reference: 0x063679ca1b28b5cfda9c186b367e271e");
    ///     }
    ///     Err(e) => log::error!("couldn't get input: {:?}", e),
    /// }
    /// ```
    pub fn input_bip39(&self, prompt: Option<&str>) -> Result<Vec<u8>, xous::Error> {
        self.lock();
        let spec = ManagedBip39 {
            token: self.token,
            caption: if let Some(c) = prompt { Some(xous_ipc::String::from_str(c)) } else { None },
            bip39_data: [0u8; 32],
            bip39_len: 0,
        };
        let mut buf = Buffer::into_buf(spec).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, Opcode::Bip39Input.to_u32().unwrap()).or(Err(xous::Error::InternalError))?;
        let result = buf.to_original::<ManagedBip39, _>().or(Err(xous::Error::InternalError))?;
        self.unlock();
        if result.bip39_len == 0 {
            Err(xous::Error::InvalidString)
        } else {
            Ok(result.bip39_data[..result.bip39_len as usize].to_vec())
        }
    }

    /// Shows image.
    /// - Blocks until the image has been dismissed.
    ///
    /// [Code Example - procedurally generate image](https://github.com/betrusted-io/xous-core/blob/c84f4efba0d4a5ae690f147c57590134d3cafc27/services/modals/src/tests.rs#L174-L201)
    /// [Code Example - show_image for procedurally generated](https://github.com/betrusted-io/xous-core/blob/c84f4efba0d4a5ae690f147c57590134d3cafc27/services/modals/src/tests.rs#L155-L163)
    ///
    /// [Code Example - show_image for downloaded image](https://github.com/betrusted-io/xous-core/blob/c84f4efba0d4a5ae690f147c57590134d3cafc27/services/shellchat/src/cmds/net_cmd.rs#L378-L483)
    #[cfg(feature = "ditherpunk")]
    pub fn show_image(&self, mut bm: Bitmap) -> Result<(), xous::Error> {
        self.lock();
        let (bm_width, bm_height) = bm.size();
        let (bm_width, bm_height) = (bm_width as u32, bm_height as u32);

        // center image in modal
        const BORDER: u32 = 3;
        let margin = Point::new(
            (BORDER + max(0, (gam::IMG_MODAL_WIDTH - 2 * BORDER - bm_width) / 2)).try_into().unwrap(),
            (BORDER + max(0, (gam::IMG_MODAL_HEIGHT - 2 * BORDER - bm_height) / 2)).try_into().unwrap(),
        );
        bm.translate(margin);

        let mut tiles: [Option<Tile>; 6] = [None; 6];
        for (t, tile) in bm.iter().enumerate() {
            if t >= tiles.len() {
                continue;
            }
            tiles[t] = Some(*tile);
        }

        let spec = ManagedImage { token: self.token, tiles };
        let buf = Buffer::into_buf(spec).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, Opcode::Image.to_u32().unwrap()).or(Err(xous::Error::InternalError))?;
        self.unlock();
        Ok(())
    }

    /// Generates progress bar (updated by update_progress, and closed by finish_progress).
    ///
    /// - This item cannot be dismissed/modified by the user.
    /// - If 'current' is less than 'start' or more than 'end', progress will show 0 or 100 percent,
    ///   respectively.
    /// - Title text wraps, burden is on the consumer not to exceed the available screen space.
    ///
    /// <details>
    ///     <summary>Example Image</summary>
    ///
    /// ![Example Image - Initial](https://github.com/rowr111/xous-core/blob/main/docs/images/modals_start_progress.png?raw=true)
    ///
    /// </details>
    ///
    /// # Example
    /// ```
    /// use modals::Modals;
    /// use xous_names::XousNames;
    /// let xns = XousNames::new().unwrap();
    /// let modals = Modals::new(&xns).unwrap();
    ///
    /// modals.start_progress("Progress Quest", 0, 1000, 0).expect("couldn't raise progress bar");
    /// ```
    pub fn start_progress(&self, title: &str, start: u32, end: u32, current: u32) -> Result<(), xous::Error> {
        self.lock();
        let spec = ManagedProgress {
            token: self.token,
            title: xous_ipc::String::from_str(title),
            start_work: start,
            end_work: end,
            current_work: current,
            user_interaction: false,
            step: 1,
        };
        let buf = Buffer::into_buf(spec).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, Opcode::StartProgress.to_u32().unwrap()).or(Err(xous::Error::InternalError))?;
        Ok(())
    }

    /// Human interaction-enabled slider.
    ///
    /// - Interactable until home/enter is pressed.
    /// - Use the D-pad to shift the slider by a step in either direction.
    /// - Note that it is possible to exceed start or end if you choose the 'step' value poorly.
    /// - Title text wraps, burden is on the consumer not to exceed available screen space.
    ///
    /// <details>
    ///     <summary>Example Image</summary>
    ///
    /// ![Example Image - Initial](https://github.com/rowr111/xous-core/blob/main/docs/images/modals_slider.png?raw=true)
    ///
    /// </details>
    ///
    /// # Example
    /// ```
    /// use modals::Modals;
    /// use xous_names::XousNames;
    /// let xns = XousNames::new().unwrap();
    /// let modals = Modals::new(&xns).unwrap();
    ///
    /// let result = modals
    ///     .slider("Human interaction-enabled slider!", 0, 100, 50, 1)
    ///     .expect("slider test failed");
    /// log::info!("result: {}", &result);
    /// ```
    pub fn slider(
        &self,
        title: &str,
        start: u32,
        end: u32,
        current: u32,
        step: u32,
    ) -> Result<u32, xous::Error> {
        self.lock();
        let spec = ManagedProgress {
            token: self.token,
            title: xous_ipc::String::from_str(title),
            start_work: start,
            end_work: end,
            current_work: current,
            user_interaction: true,
            step,
        };
        let mut buf = Buffer::into_buf(spec).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, Opcode::Slider.to_u32().unwrap()).or(Err(xous::Error::InternalError))?;

        let orig = buf.to_original::<SliderPayload, _>().unwrap();

        self.unlock();
        Ok(orig.0)
    }

    /// Updates progress bar (created by start_progress, and closed by finish_progress).
    ///
    /// - This item cannot be dismissed/modified by the user.
    /// - If you exceed 'end' of the progress bar with an update, it will show 100%.
    /// - Note that this API is not atomically token-locked, so, someone could mess with the progress bar
    ///   state. But, progress updates are meant to be fast and frequent, and generally if a progress bar
    ///   shows something whacky it's not going to affect a security outcome.
    ///
    /// - suggestions for if you run into the queue overflow error:
    ///   - preferred: reduce the rate at which updates occur by modifying the updater code to do less
    ///     frequent updates. For example, instead of once every iteration of a loop, once every ten
    ///     iterations. This will improve performance of the code and reduce UI churn as well. If this is not
    ///     possible, you can add a sleep by doing something like the code below. This will reduce the rate at
    ///     which your loop runs, as well as at which the UI updates.
    /// ```
    /// std::thread::sleep(std::time::Duration::from_millis(100)); 
    /// ```
    ///   - An alternative "softer" solution is to call yield. This will cause the producer to yield its time
    ///     slice to other processes in the OS, which can give the progress bar a chance to catch up.
    /// ```
    /// xous::yield_slice(); 
    /// ```
    ///
    /// # Example
    /// ```
    /// use modals::Modals;
    /// use xous_names::XousNames;
    /// let xns = XousNames::new().unwrap();
    /// let modals = Modals::new(&xns).unwrap();
    ///
    /// modals.start_progress("Progress Quest", 0, 1000, 0).expect("couldn't raise progress bar");
    /// modals.update_progress(10).expect("couldn't update progress bar");
    /// ```
    pub fn update_progress(&self, current: u32) -> Result<(), xous::Error> {
        match xous::try_send_message(
            self.conn,
            Message::new_scalar(Opcode::DoUpdateProgress.to_usize().unwrap(), current as usize, 0, 0, 0),
        ) {
            Ok(_) => (),
            Err(e) => {
                log::warn!("update_progress failed with {:?}, skipping request", e);
                // We got here because the modals inbound server queue has overflowed. The most likely reason
                // is that too many progress updates were sent. The yield statement below causes the sending
                // thread to sleep for the rest of its scheduling quantum, thus rate-limiting requests. The
                // current request is simply discarded; no attempt is made to retry.
                xous::yield_slice()
            }
        }
        Ok(())
    }

    /// Closes progress bar (created by start_progress, and updated by update_progress).
    ///
    ///  - Closes the progress bar, regardless of the current state.
    ///  - This is a blocking call, because you want the GAM to revert focus back to your context before you
    ///    continue with any drawing operations. Otherwise, they could be missed as the modal is still
    ///    covering your window.
    ///
    /// # Example
    /// ```
    /// use modals::Modals;
    /// use xous_names::XousNames;
    /// let xns = XousNames::new().unwrap();
    /// let modals = Modals::new(&xns).unwrap();
    ///
    /// modals.start_progress("Progress Quest", 0, 1000, 0).expect("couldn't raise progress bar");
    /// modals.update_progress(10).expect("couldn't update progress bar");
    /// modals.finish_progress().expect("couldn't dismiss progress bar");
    /// ```
    pub fn finish_progress(&self) -> Result<(), xous::Error> {
        self.lock();
        send_message(
            self.conn,
            Message::new_blocking_scalar(
                Opcode::StopProgress.to_usize().unwrap(),
                self.token[0] as usize,
                self.token[1] as usize,
                self.token[2] as usize,
                self.token[3] as usize,
            ),
        )
        .expect("couldn't stop progress");
        self.unlock();
        Ok(())
    }

    /// Creates a list to be used by get_radiobutton or get_checkbox.
    /// - Does not display on its own, the above mentioned methods prompt display of the list.
    /// - Burden is on the consumer to not exceed available screen space.
    ///
    /// # Example
    /// ```
    /// use modals::Modals;
    /// use xous_names::XousNames;
    /// let xns = XousNames::new().unwrap();
    /// let modals = Modals::new(&xns).unwrap();
    ///
    /// const LIST_TEST: [&'static str; 5] = [
    ///     "happy",
    ///     "😃",
    ///     "安",
    ///     "peace &\n tranquility",
    ///     "Once apon a time, in a land far far away, there was a",
    /// ];
    ///
    /// let items: Vec<&str> = LIST_TEST.iter().map(|s| s.to_owned()).collect();
    /// modals.add_list(items).expect("couldn't build list");
    /// ```
    pub fn add_list(&self, items: Vec<&str>) -> Result<(), xous::Error> {
        for (_, text) in items.iter().enumerate() {
            self.add_list_item(text).or(Err(xous::Error::InternalError))?;
        }
        Ok(())
    }

    /// Add individual items to a list to be used by get_radiobutton or get_checkbox.
    /// - Does not display on its own, the above mentioned methods prompt display of the list.
    ///
    /// # Example
    /// ```
    /// use modals::Modals;
    /// use xous_names::XousNames;
    /// let xns = XousNames::new().unwrap();
    /// let modals = Modals::new(&xns).unwrap();
    ///
    /// modals.add_list_item("yes").expect("failed radio yes");
    /// modals.add_list_item("no").expect("failed radio no");
    /// ```
    pub fn add_list_item(&self, item: &str) -> Result<(), xous::Error> {
        self.lock();
        let itemname = ManagedListItem { token: self.token, item: ItemName::new(item) };
        let buf = Buffer::into_buf(itemname).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, Opcode::AddModalItem.to_u32().unwrap()).or(Err(xous::Error::InternalError))?;
        Ok(())
    }

    pub fn get_radiobutton(&self, prompt: &str) -> Result<String, xous::Error> {
        self.lock();
        let spec =
            ManagedPromptWithFixedResponse { token: self.token, prompt: xous_ipc::String::from_str(prompt) };
        let mut buf = Buffer::into_buf(spec).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, Opcode::PromptWithFixedResponse.to_u32().unwrap())
            .or(Err(xous::Error::InternalError))?;
        let itemname = buf.to_original::<ItemName, _>().unwrap();
        self.unlock();
        Ok(String::from(itemname.as_str()))
    }

    pub fn get_radio_index(&self) -> Result<usize, xous::Error> {
        let msg = Message::new_blocking_scalar(Opcode::GetModalIndex.to_usize().unwrap(), 0, 0, 0, 0);
        match send_message(self.conn, msg) {
            Ok(xous::Result::Scalar1(bitfield)) => {
                let mut i = 0;
                while (i < u32::bit_length()) & !bitfield.get_bit(i) {
                    i = i + 1;
                }
                if i < u32::bit_length() { Ok(i) } else { Err(xous::Error::InternalError) }
            }
            _ => Err(xous::Error::InternalError),
        }
    }

    /// Creates modal dialog from list of items (list created by add_list or add_list_item) and returns Vector
    /// of checked items.
    /// - Dialog cannot be dismissed without pressing 'ok'.
    /// - 'ok' text is not editable.
    /// - Any or none of the checked items are acceptable to be returned.
    ///
    /// <details>
    ///     <summary>Example Image</summary>
    ///
    /// ![Example Image - Initial](https://github.com/rowr111/xous-core/blob/main/docs/images/modals_get_checkbox.png?raw=true)
    ///
    /// </details>
    ///
    /// # Example
    /// ```
    /// use modals::Modals;
    /// use xous_names::XousNames;
    /// let xns = XousNames::new().unwrap();
    /// let modals = Modals::new(&xns).unwrap();
    ///
    /// const LIST_TEST: [&'static str; 5] = [
    ///     "happy",
    ///     "😃",
    ///     "安",
    ///     "peace &\n tranquility",
    ///     "Once apon a time, in a land far far away, there was a",
    /// ];
    ///
    /// let items: Vec<&str> = LIST_TEST.iter().map(|s| s.to_owned()).collect();
    /// modals.add_list(items).expect("couldn't build list");
    /// match modals.get_checkbox("You can have it all:") {
    ///     Ok(things) => {
    ///         log::info!("The user picked {} things:", things.len());
    ///         for thing in things {
    ///             log::info!("{}", thing);
    ///         }
    ///     }
    ///     _ => log::error!("get_checkbox failed"),
    /// }
    /// ```
    pub fn get_checkbox(&self, prompt: &str) -> Result<Vec<String>, xous::Error> {
        self.lock();
        let spec =
            ManagedPromptWithFixedResponse { token: self.token, prompt: xous_ipc::String::from_str(prompt) };
        let mut buf = Buffer::into_buf(spec).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, Opcode::PromptWithMultiResponse.to_u32().unwrap())
            .or(Err(xous::Error::InternalError))?;
        let selected_items = buf.to_original::<CheckBoxPayload, _>().unwrap();
        let mut ret = Vec::<String>::new();
        for maybe_item in selected_items.payload() {
            if let Some(item) = maybe_item {
                ret.push(String::from(item.as_str()));
            }
        }
        self.unlock();
        Ok(ret)
    }

    pub fn get_check_index(&self) -> Result<Vec<usize>, xous::Error> {
        let mut ret = Vec::<usize>::new();
        let msg = Message::new_blocking_scalar(Opcode::GetModalIndex.to_usize().unwrap(), 0, 0, 0, 0);
        match send_message(self.conn, msg) {
            Ok(xous::Result::Scalar1(bitfield)) => {
                let mut i = 0;
                while i < u32::bit_length() {
                    if bitfield.get_bit(i) {
                        ret.push(i);
                    }
                    i = i + 1;
                }
                Ok(ret)
            }
            _ => Err(xous::Error::InternalError),
        }
    }

    pub fn dynamic_notification(&self, title: Option<&str>, text: Option<&str>) -> Result<(), xous::Error> {
        self.lock();
        let spec = DynamicNotification {
            token: self.token,
            title: if let Some(t) = title { Some(xous_ipc::String::from_str(t)) } else { None },
            text: if let Some(t) = text { Some(xous_ipc::String::from_str(t)) } else { None },
        };
        let buf = Buffer::into_buf(spec).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, Opcode::DynamicNotification.to_u32().unwrap())
            .or(Err(xous::Error::InternalError))?;
        Ok(())
    }

    pub fn dynamic_notification_update(
        &self,
        title: Option<&str>,
        text: Option<&str>,
    ) -> Result<(), xous::Error> {
        let spec = DynamicNotification {
            token: self.token,
            title: if let Some(t) = title { Some(xous_ipc::String::from_str(t)) } else { None },
            text: if let Some(t) = text { Some(xous_ipc::String::from_str(t)) } else { None },
        };
        let buf = Buffer::into_buf(spec).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, Opcode::UpdateDynamicNotification.to_u32().unwrap())
            .or(Err(xous::Error::InternalError))?;
        Ok(())
    }

    pub fn dynamic_notification_close(&self) -> Result<(), xous::Error> {
        send_message(
            self.conn,
            Message::new_scalar(
                Opcode::CloseDynamicNotification.to_usize().unwrap(),
                self.token[0] as usize,
                self.token[1] as usize,
                self.token[2] as usize,
                self.token[3] as usize,
            ),
        )
        .expect("couldn't stop progress");
        self.unlock();
        Ok(())
    }

    /// Blocks until we have a lock on the modals server
    fn lock(&self) {
        if !self.have_lock.get() {
            match send_message(
                self.conn,
                Message::new_blocking_scalar(
                    Opcode::GetMutex.to_usize().unwrap(),
                    self.token[0] as usize,
                    self.token[1] as usize,
                    self.token[2] as usize,
                    self.token[3] as usize,
                ),
            )
            .expect("couldn't send mutex acquisition message")
            {
                xous::Result::Scalar1(code) => {
                    if code != 1 {
                        log::warn!("Unexpected return from lock acquisition.");
                    }
                }
                _ => {
                    log::error!("Internal error trying to acquire mutex");
                }
            }
        }
        self.have_lock.set(true);
    }

    fn unlock(&self) { self.have_lock.set(false); }

    pub fn conn(&self) -> CID { self.conn }

    /// Don't leak this token outside of your server, otherwise, another server can pretend to be you and
    /// steal your modal information!
    pub fn token(&self) -> [u32; 4] { self.token }
}

use core::sync::atomic::{AtomicU32, Ordering};
static REFCOUNT: AtomicU32 = AtomicU32::new(0);
impl Drop for Modals {
    fn drop(&mut self) {
        // the connection to the server side must be reference counted, so that multiple instances of this
        // object within a single process do not end up de-allocating the CID on other threads before
        // they go out of scope. Note to future me: you want this. Don't get rid of it because you
        // think, "nah, nobody will ever make more than one copy of this object".
        if REFCOUNT.fetch_sub(1, Ordering::Relaxed) == 1 {
            unsafe {
                xous::disconnect(self.conn).unwrap();
            }
        }
        // if there was object-specific state (such as a one-time use server for async callbacks, specific to
        // the object instance), de-allocate those items here. They don't need a reference count
        // because they are object-specific
    }
}

/// If a dynamic notification is active, this will block and return only if one of two
/// conditions are met:
/// 1. a key is pressed, in which case, the `Some(char)` is the key pressed. If there is a "fat finger" event,
///    only the first character is reported.
/// 2. the dynamic notification is closed, in which case a `None` is reported.
///
/// This function is "broken out" so that it can be called from a thread without having
/// to wrap a mutex around the primary Modals structure.
pub fn dynamic_notification_blocking_listener(
    token: [u32; 4],
    conn: CID,
) -> Result<Option<char>, xous::Error> {
    match send_message(
        conn,
        Message::new_blocking_scalar(
            Opcode::ListenToDynamicNotification.to_usize().unwrap(),
            token[0] as usize,
            token[1] as usize,
            token[2] as usize,
            token[3] as usize,
        ),
    )
    .expect("couldn't listen")
    {
        xous::Result::Scalar2(is_some, code) => {
            if is_some == 1 {
                let c = char::from_u32(code as u32).unwrap_or('\u{0000}');
                Ok(Some(c))
            } else if is_some == 2 {
                log::warn!("Attempt to listen, but did not have the mutex. Aborted.");
                Ok(None)
            } else {
                Ok(None)
            }
        }
        _ => Err(xous::Error::InternalError),
    }
}
