mod group_permission;
mod link_state;
mod trust_mode;

use crate::Account;
use group_permission::GroupPermission;
use link_state::LinkState;
use std::io::Error;
pub use trust_mode::TrustMode;

// Structure modeled on signal-cli by AsamK <asamk@gmx.de> and contributors - https://github.com/AsamK/signal-cli.
// signal-cli is a commandline interface for libsignal-service-java. It supports registering, verifying, sending and receiving messages.
// see signal-cli Manual Page - https://github.com/AsamK/signal-cli/blob/master/man/signal-cli.1.adoc

#[allow(dead_code)]
pub struct Manager {
    account:Account,
    trust_mode: TrustMode,
    log_verbose: bool,
    log_scrub: bool,
    log_send: bool,
}

impl Manager {
    /// Create a new Signal account
    ///
    /// # Arguments
    /// * `live` - Specify the server environment
    /// * `account` - Specify your phone number, that will be your identifier. The phone number must include the country calling code, i.e. the number must start with a "+" sign.
    ///
    pub fn new(account: Account, trust_mode: TrustMode) -> Manager {
        Manager {
            account,
            trust_mode,
            log_verbose: false,
            log_scrub: false,
            log_send: false,
        }
    }

    /// Register a phone number with SMS or voice verification. Use the verify command to complete the verification.
    ///
    /// For registering you need a phone number where you can receive SMS or incoming calls.
    ///
    /// If the account is just deactivated, the register command will just reactivate account, without requiring an SMS verification. By default the unregister command just deactivates the account, in which case it can be reactivated without sms verification if the local data is still available. If the account was deleted (with --delete-account) it cannot be reactivated.
    ///
    /// # Arguments
    /// * `voice` - The verification should be done over voice, not SMS.
    /// * `captcha` - The captcha token, required if registration failed with a captcha required error. To get the token, go to https://signalcaptchas.org/registration/generate.html For the staging environment, use: https://signalcaptchas.org/staging/registration/generate.html Check the developer tools for a redirect starting with signalcaptcha:// Everything after signalcaptcha:// is the captcha token.
    ///
    #[allow(dead_code)]
    pub fn register(&mut self, _voice: bool, _captcha: Option<&str>) -> Result<(), Error> {
        todo!();
    }

    /// Verify the number using the code received via SMS or voice.
    ///
    /// * `verification_code` - The verification code.
    /// * `pin` - The registration lock PIN, that was set by the user. Only required if a PIN was set.
    ///
    #[allow(dead_code)]
    pub fn verify(_verification_code: &str, _pin: Option<&str>) -> Result<(), Error> {
        todo!();
    }

    /// Disable push support for this device, i.e. this device won’t receive any more messages. If this is the primary device, other users can’t send messages to this number anymore. Use "updateAccount" to undo this. To remove a linked device, use "removeDevice" from the primary device.
    ///
    /// # Arguments
    /// * `delete_account` - Delete account completely from server. Cannot be undone without loss. You will have to be readded to each group.
    ///
    /// Caution: Only delete your account if you won’t use this number again!
    ///
    #[allow(dead_code)]
    pub fn unregister(_delete_account: bool) -> Result<(), Error> {
        todo!();
    }

    /// Update the account attributes on the signal server. Can fix problems with receiving messages.
    ///
    /// # Arguments
    /// * `name` - Set a new device name for the primary or linked device
    ///
    #[allow(dead_code)]
    pub fn update_account(_name: &str) -> Result<(), Error> {
        todo!();
    }

    /// Update signal configs and sync them to linked devices. This command only works on the primary devices.
    ///
    /// # Arguments
    /// * `read_receipts` - Indicates if Signal should send read receipts.
    /// * `unidentified_delivery_indicators` - Indicates if Signal should show unidentified delivery indicators.
    /// * `typing_indicators` - Indicates if Signal should send/show typing indicators.
    /// * `link_previews` - Indicates if Signal should generate link previews.
    ///
    #[allow(dead_code)]
    pub fn update_configuration(
        _read_receipts: bool,
        _unidentified_delivery_indicators: bool,
        _typing_indicators: bool,
        _link_previews: bool,
    ) -> Result<(), Error> {
        todo!();
    }

    /// Set a registration lock pin, to prevent others from registering this number.
    ///
    /// # Arguments
    /// * `registration_lock_pin` - The registration lock PIN, that will be required for new registrations (resets after 7 days of inactivity)
    ///
    #[allow(dead_code)]
    pub fn set_pin(_registration_lock_pin: &str) -> Result<(), Error> {
        todo!();
    }

    /// Remove the registration lock pin.
    ///
    #[allow(dead_code)]
    pub fn remove_pin() -> Result<(), Error> {
        todo!();
    }

    /// Link to an existing device, instead of registering a new number. This shows a "sgnl://linkdevice?uuid=…​" URI. If you want to connect to another signal-cli instance, you can just use this URI. If you want to link to an Android/iOS device, create a QR code with the URI (e.g. with qrencode) and scan that in the Signal app.
    ///
    /// # Arguments
    /// * `name` - Optionally specify a name to describe this new device (defaults to "xous").
    /// * `host` - Optionally specify a host to connect to (defaults to "signal.org").
    ///
    #[allow(dead_code)]
    pub fn link(&mut self, name: Option<&str>, host: Option<&str>) -> Result<bool, Error> {
        todo!();
    }

    /// Link another device to this device. Only works, if this is the primary device.
    ///
    /// # Arguments
    /// `uri` - Specify the uri contained in the QR code shown by the new device. You will need the full URI such as "sgnl://linkdevice?uuid=…​" (formerly "tsdevice:/?uuid=…​") Make sure to enclose it in quotation marks for shells.
    ///
    #[allow(dead_code)]
    pub fn add_device(_uri: &str) -> Result<(), Error> {
        todo!();
    }

    /// Show a list of linked devices.
    ///
    #[allow(dead_code)]
    pub fn list_devices() -> Result<Vec<String>, Error> {
        todo!();
    }

    /// Remove a linked device. Only works, if this is the primary device.
    ///
    /// # Arguments
    /// * `device_id` - Specify the device you want to remove. Use listDevices to see the deviceIds.
    ///
    #[allow(dead_code)]
    pub fn remove_device(_device_id: &str) -> Result<(), Error> {
        todo!();
    }

    /// Uses a list of phone numbers to determine the statuses of those users. Shows if they are registered on the Signal Servers or not. In json mode this is outputted as a list of objects.
    ///
    /// # Arguments
    /// * `recipient` - One or more numbers to check.
    ///
    #[allow(dead_code)]
    pub fn get_user_status(_recipient: Vec<&str>) -> Result<(), Error> {
        todo!();
    }

    /// Send a message to another user or group.
    ///
    /// # Arguments
    /// * `recipient` - Specify the recipients’ phone number.
    /// * `note_to_self` - Send the message to self without notification.
    /// * `group_id` - Specify the recipient group ID in base64 encoding.
    /// * `message` - Specify the message
    /// * `attachments` -Add one or more files as attachment. Can be either a file path or a data URI. Data URI encoded attachments must follow the RFC 2397. Additionally a file name can be added: e.g.: data:<MIME-TYPE>;filename=<FILENAME>;base64,<BASE64 ENCODED DATA>
    /// * `sticker` - Send a sticker of a locally known sticker pack (syntax: stickerPackId:stickerId). Shouldn’t be used together with -m as the official clients don’t support this. e.g.: --sticker 00abac3bc18d7f599bff2325dc306d43:2
    /// * `mention` - Mention another group member (syntax: start:length:recipientNumber) In the apps the mention replaces part of the message text, which is specified by the start and length values. e.g.: -m "Hi X!" --mention "3:1:+123456789"
    /// * `text_style` - Style parts of the message text (syntax: start:length:STYLE). Where STYLE is one of: BOLD, ITALIC, SPOILER, STRIKETHROUGH, MONOSPACE
    /// * `quote_timestamp` - Specify the timestamp of a previous message with the recipient or group to add a quote to the new message.
    /// * `quote_author` - Specify the number of the author of the original message.
    /// * `quote_message` - Specify the message of the original message.
    /// * `quote_mention` - Specify the mentions of the original message (same format as --mention).
    /// * `quote_text_style` - Style parts of the original message text (same format as --text-style).
    /// * `preview_url` - Specify the url for the link preview. The same url must also appear in the message body, otherwise the preview won’t be displayed by the apps.
    /// * `preview_title` - Specify the title for the link preview (mandatory).
    /// * `preview_description` - Specify the description for the link preview (optional).
    /// * `preview_image` - Specify the image file for the link preview (optional).
    /// * `story_timestamp` - Specify the timestamp of a story to reply to.
    /// * `story_author` - Specify the number of the author of the story.
    /// * `end_session` - Clear session state and send end session message.
    /// * `edit_timestamp` - Specify the timestamp of a previous message with the recipient or group to send an edited message.
    ///
    #[allow(dead_code)]
    pub fn send(
        _recipient: Vec<&str>,
        _note_to_self: bool,
        _group_id: Vec<&str>,
        _message: Option<&str>,
        _attachments: Vec<&str>,
        _sticker: Option<&str>,
        _mention: Option<&str>,
        _text_style: Option<&str>,
        _quote_timestamp: Option<u64>,
        _quote_author: Option<&str>,
        _quote_message: Option<&str>,
        _quote_mention: Vec<&str>,
        _quote_text_style: Option<&str>,
        _preview_url: Option<&str>,
        _preview_title: Option<&str>,
        _preview_description: Option<&str>,
        _preview_image: Option<&str>,
        _story_timestamp: Option<u64>,
        _story_author: Option<&str>,
        _end_session: bool,
        _edit_timestamp: Option<u64>,
    ) -> Result<(), Error> {
        todo!();
    }

    /// Send a payment notification.
    ///
    /// # Arguments
    /// `recipient` - Specify the recipient’s phone number.
    /// `receipt` - The base64 encoded receipt blob.
    /// `note` - Specify a note for the payment notification.
    ///
    #[allow(dead_code)]
    pub fn send_payment_notification(
        _recipient: &str,
        _receipt: &str,
        _note: &str,
    ) -> Result<(), Error> {
        todo!();
    }

    /// Send reaction to a previously received or sent message.
    ///
    /// # Arguments
    /// * `recipient` - Specify the recipients’ phone number.
    /// * `group_id` - Specify the recipient group ID in base64 encoding.
    /// * `emoji` - Specify the emoji, should be a single unicode grapheme cluster.
    /// * `target_author` - Specify the number of the author of the message to which to react.
    /// * `target_timestamp` - Specify the timestamp of the message to which to react.
    /// * `remove` - Remove a reaction.
    /// * `story` - React to a story instead of a normal message
    ///
    #[allow(dead_code)]
    pub fn send_reaction(
        _recipient: Vec<&str>,
        _group_id: Vec<&str>,
        _emoji: &str,
        _target_author: &str,
        _target_timestamp: u64,
        _remove: bool,
        _story: bool,
    ) -> Result<(), Error> {
        todo!();
    }

    /// Send a read or viewed receipt to a previously received message.
    ///
    /// # Arguments
    /// * `recipient` - Specify the sender’s phone number.
    /// * `target_timestamp - Specify the timestamp of the message to which to react.
    /// * `reciept_type` - Specify the receipt type, either read (the default) or viewed.
    ///
    #[allow(dead_code)]
    pub fn send_receipt(
        _recipient: &str,
        _target_timestamp: Vec<u64>,
        _receipt_type: &str,
    ) -> Result<(), Error> {
        todo!();
    }

    // Send typing message to trigger a typing indicator for the recipient. Indicator will be shown for 15seconds unless a typing STOP message is sent first.
    ///
    /// # Arguments
    /// * `recipient` - Specify the sender’s phone number.
    /// * `group_id` - Specify the recipient group ID in base64 encoding.
    /// * `stop` - Send a typing STOP message.
    ///
    #[allow(dead_code)]
    pub fn send_typing(
        _recipient: Vec<&str>,
        _group_id: Vec<&str>,
        _stop: bool,
    ) -> Result<(), Error> {
        todo!();
    }

    /// Remotely delete a previously sent message.
    ///
    /// # Arguments
    /// * `recipient` - Specify the sender’s phone number.
    /// * `group_id` - Specify the recipient group ID in base64 encoding.
    /// * `target_timestamp` - Specify the timestamp of the message to which to react.
    ///
    #[allow(dead_code)]
    pub fn remote_delete(
        _recipient: Vec<&str>,
        _group_id: Vec<&str>,
        _target_timestamp: u64,
    ) -> Result<(), Error> {
        todo!();
    }

    /// Query the server for new messages. New messages are printed on standard output and attachments are downloaded to the config directory. In json mode this is outputted as one json object per line.
    ///
    /// # Arguments
    /// * `timeout` - Number of seconds to wait for new messages (negative values disable timeout). Default is 5 seconds.
    /// * `max-messages` - Maximum number of messages to receive, before returning.
    /// * `ignore-attachments` - Don’t download attachments of received messages.
    /// * `ignore-stories` - Don’t receive story messages from the server.
    /// * `send-read-receipts` - Send read receipts for all incoming data messages (in addition to the default delivery receipts)
    ///
    #[allow(dead_code)]
    pub fn receive(
        _timeout: f64,
        _max_messages: u16,
        _ignore_attachments: bool,
        _ignore_stories: bool,
        _send_read_receipts: bool,
    ) -> Result<(), Error> {
        todo!();
    }

    /// Join a group via an invitation link.
    ///
    /// # Arguments
    /// * `uri` - The invitation link URI (starts with https://signal.group/#)
    ///
    #[allow(dead_code)]
    pub fn join_group(_uri: &str) -> Result<(), Error> {
        todo!();
    }

    /// Create or update a group. If the user is a pending member, this command will accept the group invitation.
    ///
    /// # Arguments
    /// * `group_id' - Specify the recipient group ID in base64 encoding. If not specified, a new group with a new random ID is generated.
    /// * `name' - Specify the new group name.
    /// * `description' - Specify the new group description.
    /// * `avatar' - Specify a new group avatar image file.
    /// * `member' - Specify one or more members to add to the group.
    /// * `remove_member' - Specify one or more members to remove from the group
    /// * `admin ' - Specify one or more members to make a group admin
    /// * `remove_admin' - Specify one or more members to remove group admin privileges
    /// * `ban' - Specify one or more members to ban from joining the group. Banned members cannot join or request to join via a group link.
    /// * `unban' - Specify one or more members to remove from the ban list
    /// * `reset_link' - Reset group link and create new link password
    /// * `link' - Set group link state: enabled, enabled-with-approval, disabled
    /// * `set_permission_add_member' - Set permission to add new group members: every-member, only-admins
    /// * `set_permission_edit_details' - Set permission to edit group details: every-member, only-admins
    /// * `set_permission_send_messages' - Set permission to send messages in group: every-member, only-admins Groups where only admins can send messages are also called announcement groups
    /// * `expiration' - Set expiration time of messages (seconds). To disable expiration set expiration time to 0.
    ///
    #[allow(dead_code)]
    pub fn update_group(
        _group_id: Option<&str>,
        _name: Option<&str>,
        _description: Option<&str>,
        _avatar: Option<&str>,
        _member: Vec<&str>,
        _remove_member: Vec<&str>,
        _admin: Vec<&str>,
        _remove_admin: Vec<&str>,
        _ban: Vec<&str>,
        _unban: Vec<&str>,
        _reset_link: bool,
        _link: Option<LinkState>,
        _set_permission_add_member: Option<GroupPermission>,
        _set_permission_edit_details: Option<GroupPermission>,
        _set_permission_send_messages: Option<GroupPermission>,
        _expiration: Option<u32>,
    ) -> Result<(), Error> {
        todo!();
    }

    /// Send a quit group message to all group members and remove self from member list. If the user is a pending member, this command will decline the group invitation.
    ///
    /// # Aurguments
    /// * `group_id` - Specify the recipient group ID in base64 encoding.
    /// * `delete` - Delete local group data completely after quitting group.
    ///
    #[allow(dead_code)]
    pub fn quit_group(_group_id: &str, _delete: bool) -> Result<(), Error> {
        todo!();
    }

    // Show a list of known groups and related information. In json mode this is outputted as an list of objects and is always in detailed mode.
    ///
    /// # Arguments
    /// * `detailed` - Include the list of members of each group and the group invite link.
    /// * `group_id` - Filter the group list by one or more group IDs.
    ///
    #[allow(dead_code)]
    pub fn list_groups(_detailed: bool, _group_id: Vec<&str>) -> Result<(), Error> {
        todo!();
    }

    // Show a list of known contacts with names and profiles. When a specific recipient is given, its profile will be refreshed.
    ///
    /// # Arguments
    /// * `recipient` - Specify the recipients’ phone number.
    /// * `all_recipients` - Include all known recipients, not only contacts.
    /// * `blocked` - Specify if only blocked or unblocked contacts should be shown (default: all contacts)
    /// * `name` - Find contacts with the given contact or profile name.
    ///
    #[allow(dead_code)]
    pub fn list_contacts(
        _recipient: &str,
        _all_recipients: bool,
        _blocked: bool,
        _name: &str,
    ) -> Result<(), Error> {
        todo!();
    }

    // List all known identity keys and their trust status, fingerprint and safety number.
    ///
    /// # Arguments
    /// * `number` - Only show identity keys for the given phone number.
    ///
    #[allow(dead_code)]
    pub fn list_identities(_number: &str) -> Result<(), Error> {
        todo!();
    }

    /// Set the trust level of a given number. The first time a key for a number is seen, it is trusted by default (TOFU). If the key changes, the new key must be trusted manually.
    ///
    /// # Arguments
    /// * `number` - Specify the phone number, for which to set the trust.
    /// * `trust_all_known_keys` - Trust all known keys of this user, only use this for testing.
    /// * `verified_safety_number` - Specify the safety number of the key, only use this option if you have verified the safety number. Can be either the plain text numbers shown in the app or the bytes from the QR-code, encoded as base64.
    ///
    #[allow(dead_code)]
    pub fn trust(
        _recipient: &str,
        _trust_all_known_keys: bool,
        _verified_safety_number: Option<&str>,
    ) -> Result<(), Error> {
        todo!();
    }

    // Update the profile information shown to message recipients. The profile is stored encrypted on the Signal servers. The decryption key is sent with every outgoing messages to contacts and included in every group.
    ///
    /// # Arguments
    /// * `given_name` - New (given) name.
    /// * `family_name` - New family name.
    /// * `about` - New profile status text.
    /// * `about_emoji` - New profile status emoji.
    /// * `avatar` - Path to the new avatar image file.
    /// * `remove_avatar` - Remove the avatar
    /// * `mobile_coin_address` - New MobileCoin address (Base64 encoded public address)
    ///
    #[allow(dead_code)]
    pub fn update_profile(
        _given_name: Option<&str>,
        _family_name: Option<&str>,
        _about: Option<&str>,
        _about_emoji: Option<&str>,
        _avatar: Option<&str>,
        _remove_avatar: bool,
        _mobile_coin_address: Option<&str>,
    ) -> Result<(), Error> {
        todo!();
    }

    /// Update the info associated to a number on our contact list. This change is only local but can be synchronized to other devices by using sendContacts (see below). If the contact doesn’t exist yet, it will be added.
    ///
    /// # Arguments
    /// * `number` - Specify the contact phone number.
    /// * `given_name` - New (given) name.
    /// * `family_name` - New family name.
    /// * `expiration` - Set expiration time of messages (seconds). To disable expiration set expiration time to 0.
    ///
    #[allow(dead_code)]
    pub fn update_contact(
        _given_name: Option<&str>,
        _family_name: Option<&str>,
        _expiration: Option<u32>,
    ) -> Result<(), Error> {
        todo!();
    }

    // Remove the info of a given contact
    ///
    /// # Arguments
    /// * `number` - Specify the contact phone number.
    /// * `forget` - Delete all data associated with this contact, including identity keys and sessions.
    /// * `block` - Block the given contacts or groups (no messages will be received). This change is only local but can be synchronized to other devices by using sendContacts (see below).
    /// * `contacts` - Specify the phone numbers of contacts that should be blocked.
    /// * `group_ids` - Specify the group IDs that should be blocked in base64 encoding.
    ///
    #[allow(dead_code)]
    pub fn remove_contact(
        _number: &str,
        _forget: bool,
        _block: bool,
        _contacts: Vec<&str>,
        _group_ids: Vec<&str>,
    ) -> Result<(), Error> {
        todo!();
    }

    // Unblock the given contacts or groups (messages will be received again). This change is only local but can be synchronized to other devices by using sendContacts (see below).
    ///
    /// # Arguments
    /// * `contacts` - Specify the phone numbers of contacts that should be unblocked.
    /// * `group_ids` - Specify the group IDs that should be unblocked in base64 encoding.
    ///
    #[allow(dead_code)]
    pub fn unblock(_contacts: Vec<&str>, _group_ids: Vec<&str>) -> Result<(), Error> {
        todo!();
    }

    /// Send a synchronization message with the local contacts list to all linked devices. This command should only be used if this is the primary device.
    #[allow(dead_code)]
    pub fn send_contacts() -> Result<(), Error> {
        todo!();
    }

    /// Send a synchronization request message to the primary device (for group, contacts, …​). The primary device will respond with synchronization messages with full contact and group lists.
    ///
    #[allow(dead_code)]
    pub fn send_sync_request() -> Result<(), Error> {
        todo!();
    }

    /// Upload a new sticker pack, consisting of a manifest file and the sticker images.
    ///
    /// Images must conform to the following specification: (see https:///support.signal.org/hc/en-us/articles/360031836512-Stickers#sticker_reqs )
    /// * Static stickers in PNG or WebP format
    /// * Animated stickers in APNG format,
    /// * Maximum file size for a sticker file is 300KiB
    /// * Image resolution of 512 x 512 px
    ///
    /// The required manifest.json has the following format:
    ///
    /// {
    ///   "title": "<STICKER_PACK_TITLE>",
    ///   "author": "<STICKER_PACK_AUTHOR>",
    ///   "cover": { // Optional cover, by default the first sticker is used as cover
    ///     "file": "<name of image file, mandatory>",
    ///     "contentType": "<optional>",
    ///     "emoji": "<optional>"
    ///   },
    ///   "stickers": [
    ///     {
    ///       "file": "<name of image file, mandatory>",
    ///       "contentType": "<optional>",
    ///       "emoji": "<optional>"
    ///     }
    ///     ...
    ///   ]
    /// }
    ///
    /// # Arguments
    /// * `path` - The path of the manifest.json or a zip file containing the sticker pack you wish to upload.
    ///
    #[allow(dead_code)]
    pub fn upload_sticker_pack(_path: &str) -> Result<(), Error> {
        todo!();
    }

    /// Show a list of known sticker packs.
    ///
    #[allow(dead_code)]
    pub fn list_sticker_packs() -> Result<Vec<String>, Error> {
        todo!();
    }

    /// Install a sticker pack for this account.
    ///
    /// # Arguments
    /// * `uri` - Specify the uri of the sticker pack. e.g. https://signal.art/addstickers/#pack_id=XXX&pack_key=XXX)"
    ///
    #[allow(dead_code)]
    pub fn add_sticker_pack(_uri: &str) -> Result<(), Error> {
        todo!();
    }

    /// Gets the raw data for a specified attachment. This is done using the ID of the attachment the recipient or group ID. The attachment data is returned as a Base64 String.
    ///
    /// # Arguments
    /// * `id` - The ID of the attachment as given in the attachment list of the message.
    /// * `recipient` - Specify the number which sent the attachment. Referred to generally as recipient.
    /// * `group_id` - Alternatively, specify the group IDs for which to get the attachment.
    ///
    #[allow(dead_code)]
    pub fn get_attachment(_recipient: Option<&str>, _group_id: Option<&str>) -> Result<(), Error> {
        todo!();
    }

    // When running into rate limits, sometimes the limit can be lifted, by solving a CAPTCHA. To get the captcha token, go to https://signalcaptchas.org/challenge/generate.html For the staging environment, use: https://signalcaptchas.org/staging/registration/generate.html
    ///
    /// # Arguments
    /// * `challenge` - The challenge token from the failed send attempt.
    /// * `captcha` - The captcha result, starting with signalcaptcha://
    ///
    #[allow(dead_code)]
    pub fn submit_rate_limit_challenge(_challenge: &str, _captcha: &str) -> Result<(), Error> {
        todo!();
    }

    /// Configure logging.
    ///
    /// # Arguments
    /// * `verbose` - Raise log level and include lib signal logs.
    /// * `scrub` - Scrub possibly sensitive information from the log, like phone numbers and UUIDs.
    /// * `send_log` - Enable message send log (for resending messages that recipient couldn’t decrypt).
    ///
    #[allow(dead_code)]
    pub fn log_config(_verbose: bool, _scrub: bool, _send_log: bool) -> Result<(), Error> {
        todo!();
    }

    /// Set the trust mode
    ///
    /// * `trust_mode` - Specify when to trust new identities
    ///
    #[allow(dead_code)]
    pub fn trust_mode_set(&mut self, trust_mode: TrustMode) {
        self.trust_mode = trust_mode;
    }

    /// Version
    ///
    /// Return the version.
    ///
    #[allow(dead_code)]
    pub fn version() -> Result<String, Error> {
        todo!();
    }
}
