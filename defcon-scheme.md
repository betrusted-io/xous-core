# Definitions

Let's model defcon as a game where you have players that want to be honest, and those who want to be cheaters.
In the spirit of defcon, we facilitate the cheaters agenda, but we also want the game to still be
interesting for honest players that want to play it.

The "Game" is to collect all the colors & patterns of LEDs from everyone else. The patterns are coded
in a 20-byte record that is transferred via QR codes. The light pattern itself is stored as
a diploid "genome", so mixing of light patterns is not deterministic and can evolve unique patterns
through some induced randomness in the exchange protocol.

A "cheater" would put their device in developer mode, and can run arbitrary code on it. in this case,
they can display any pattern they want. The game loses all value since they can just make any pattern
happen. Theres' nothing I can do (or want to do) to prevent that; people are encouraged to show
off their light pattern coding skills.

However, to facilitate the "honest" game, upon entry to developer mode, we can assume that
master keys are wiped, and thus "cheaters" lose access to all of the in-game secrets.
In the case that someone can get their device into developer mode without erasing their secrets,
*I* win, so long as they share their method with me - I get a free security audit.

OK, given these assumptions, let's assume that each device can keep a secret, and upon transition
to developer mode, the secrets are wiped.

Honest players can prove their honesty by interacting with an "oracle" that I would provide at
the conference, which they can only talk to only if they still have their secret data intact.

A second aspect of the "game" is that I want brute force to be a potentially legitimate option to "win",
so I need to pick a cryptosystem whose strength can be tuned based on a parameter.

In this context, I think the one thing I want to avoid is someone simply posting a QR code of their
pattern on-line, so that honest players can acquiring it by scanning that code.

In other words, the goal of the system is to make the QR codes one-time use, and we can assume
protocol integrity is enforced by honest firmware running on honest devices.

# Scheme & Keys

Assume a symmetric cipher based on AES-GCM-SIV (256). An encryption operation is denoted by (C, tag) = AES256E(K, N, M, AD),
where K is the key, N is the nonce, M is the message, AD is the associated data; and C is the ciphertext and t is
the authentication tag. Decryption is denoted by (M, valid) = AES256D(K, N, C || tag, AD), where valid is a flag
which indicates if the decryption was valid or not.

Each badge has a key that is composed as follows:

`K = Ko || Kp`, where `length(K) = 256`

- Ko is a shared secret among all the badges, which is erased upon entry to developer mode. The
length of Ko is envisioned to be around 96 bits - brute-forceable, but hard.
- Kp is a publicly disclosed portion of the secret. The length of Kp is tuned to make Ko easier -
in other words, perhaps every day of the conference, a few more bits of Ko are leaked, increasing
the size of Kp.

Ko = 96 => Kp = 160 bits are disclosed to start with.

QR codes are limited to 40 bytes of data.

# Light-pattern exchange protocol

Bob sees Alice's lights and decides he wants to incorporate her patterns.

## Phase 1: Consent

  `'←' | '→'` -> `VaultMode::ShowKey` on recipient badge

Bob asks Alice if he can "breed" with her lights. As part of the request, Bob shows
a QR code on his badge that reveals

`header || Nonce1`

`Header` is a fixed pre-amble used to identify this phase of the protocol, and is about 16 bytes long.
`Nonce1` is 12 bytes long, and randomly generated, with a small bias: a check is made to ensure the
newly generated value is always distinct from the previous one, and distinct from the pre-amble.
If by pure chance a repeat occurs, the random number generator is run again.

  `🔥` key on donor -> `VaultMode::ResponseGene` on donor badge

## Phase 2: Genetic Transfer

  Any button on recipient -> `🔥` scanning state
  Any button on donor -> `VaultMode::Idle`

Alice affirms consent by scanning Bob's nonce. In response, her badge now shows a QR code that contains

`AES256E( K, Nonce1, light-pattern-bob || pad || badge_type, null ) || tag`

`light-pattern` is 9 bytes long. This leaves a few bytes of
margin, which will probably be absorbed by adding features to the `light-pattern` record.
The bytes of margin are represented by `pad` and are defined to be 0 but are ignored
by the receiver either way.

Bob then scan's Alice's badge and can now perform

`AES256D( K, Nonce1, C || tag, null)` to derive light-pattern. The pattern is only accepted
if the MAC checks out.

  After successful scan -> `VaultMode::ConfirmGene` menu & state

## Observations

The scheme does nothing to protect the secrecy of the light pattern data. Everyone has most
of the key, and the Nonce is shared in the clear, and can in fact be photographed
and posted somewhere.

However, honest actors running signed code can only decrypt QR codes that have been correctly
encrypted to the Nonce they presented in that transaction. Thus, someone simply posting their QR
code, as encrypted to one transaction, is unlikely to enable 'honest users' to cheat.

In the worst case, someone could post their `header || Nonce1` phase on social media, and ask
someone to scan it and respond with a code that they can then scan. This is solved by forcing
the protocol to time-out within one minute, thus bounding the response window to near real-time
transactions.

Extraction of Ko requires a break of the badge security. Effectively, Ko is a "flag" that
is there to be captured - it can be captured either through brute force, or by finding a vulnerability
in the badge's security. Badges that go into developer mode will erase the master key that
protects Ko, and thus they will lose the ability to participate in any handshakes with other
badges.

**Security proposition 1**

Without Ko disclosed, I there is no practical way for an honest person to acquire a light
pattern short of doing an exchange with someone else, or a brute force attack.

**Security proposition 2**
An honest user cannot repeatedly scan a QR code and continue to "breed" with the pattern,
because the nonce is guaranteed to change on every round due to the difference check on the nonce.

**Security proposition 2**

Any copy of a QR code from a transaction in progress is unlikely to be useful to any honest
user.

**Limitation 1**

With Ko disclosed, someone could make an interactive phone app that allows someone to generate
arbitrary light patterns and complete a handshake with the app.

**Observation 1**

The security of the system can be dynamically "tuned down" by releasing more bits of Kp over time,
but more or less the whole system hinges on a common shared secret, Ko, remaining secret across
all the badges. This is normally not a very desireable property but in the context of getting
free red-teaming from world-class security experts, I feel like this might actually be...appropriate?

Releasing more bits of Kp does not require a change to the protocol, or badge firmware. This is
simply done by proclamation on social media. The proposed strength schedule, by day is:

- Day 1: 96 bits
- Day 2: 64 bits
- Day 3: 56 bits
- Day 4: 48 bits

The sharp drop on day 4 is intended to test the null hypothesis that anyone would even attempt to
brute-force the key. 48 bits should be thoroughly brute-forced on a system that can check
a billion keys per second in about 3 days time. For reference:

  - Desktop CPU is estimated to do 100M keys/s - 30 days
  - RTX4090 is estimated to do 3000M keys/s - 1 day
  - FPGA cracker is estimated to do about 4000M keys/s - 12 hours

Thus a single RTX4090 should be able to crack 48 bits of key in about 1 day's time. The 56-bit
key should be crackable by someone in 2 days if they have access to a farm of 128 GPUs, so it's
feasible for someone with a credit card, AWS access, and a few thousand bucks to burn. The 64 bit key
should require a farm of 60,000 GPUs running flat-out for a couple days straight - possible, but that's
about 500MWh of energy - about $25k-$50k in energy alone, not to mention the cloud tenancy fees.
Within the range of a very rich organization doing it just to show off their capability, but unlikely.

Breaking at 96 bits strength would require disclosure of as of yet unknown technology or techniques.