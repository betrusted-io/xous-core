#! /usr/bin/env python3

import argparse

import usb.core
import usb.util
import array
import sys
import hashlib
import csv
import urllib.request
import re

from progressbar.bar import ProgressBar
from Crypto.Cipher import AES
from Crypto.Hash import SHA256
from Crypto.Random import get_random_bytes

def bip39_to_bits(phrase):
    BIP39_TABLE_EN = [
        "abandon", "ability", "able", "about", "above", "absent", "absorb",
        "abstract", "absurd", "abuse", "access", "accident", "account",
        "accuse", "achieve", "acid", "acoustic", "acquire", "across", "act",
        "action", "actor", "actress", "actual", "adapt", "add", "addict",
        "address", "adjust", "admit", "adult", "advance", "advice", "aerobic",
        "affair", "afford", "afraid", "again", "age", "agent", "agree",
        "ahead", "aim", "air", "airport", "aisle", "alarm", "album",
        "alcohol", "alert", "alien", "all", "alley", "allow", "almost",
        "alone", "alpha", "already", "also", "alter", "always", "amateur",
        "amazing", "among", "amount", "amused", "analyst", "anchor",
        "ancient", "anger", "angle", "angry", "animal", "ankle", "announce",
        "annual", "another", "answer", "antenna", "antique", "anxiety", "any",
        "apart", "apology", "appear", "apple", "approve", "april", "arch",
        "arctic", "area", "arena", "argue", "arm", "armed", "armor", "army",
        "around", "arrange", "arrest", "arrive", "arrow", "art", "artefact",
        "artist", "artwork", "ask", "aspect", "assault", "asset", "assist",
        "assume", "asthma", "athlete", "atom", "attack", "attend", "attitude",
        "attract", "auction", "audit", "august", "aunt", "author", "auto",
        "autumn", "average", "avocado", "avoid", "awake", "aware", "away",
        "awesome", "awful", "awkward", "axis", "baby", "bachelor", "bacon",
        "badge", "bag", "balance", "balcony", "ball", "bamboo", "banana",
        "banner", "bar", "barely", "bargain", "barrel", "base", "basic",
        "basket", "battle", "beach", "bean", "beauty", "because", "become",
        "beef", "before", "begin", "behave", "behind", "believe", "below",
        "belt", "bench", "benefit", "best", "betray", "better", "between",
        "beyond", "bicycle", "bid", "bike", "bind", "biology", "bird",
        "birth", "bitter", "black", "blade", "blame", "blanket", "blast",
        "bleak", "bless", "blind", "blood", "blossom", "blouse", "blue",
        "blur", "blush", "board", "boat", "body", "boil", "bomb", "bone",
        "bonus", "book", "boost", "border", "boring", "borrow", "boss",
        "bottom", "bounce", "box", "boy", "bracket", "brain", "brand",
        "brass", "brave", "bread", "breeze", "brick", "bridge", "brief",
        "bright", "bring", "brisk", "broccoli", "broken", "bronze", "broom",
        "brother", "brown", "brush", "bubble", "buddy", "budget", "buffalo",
        "build", "bulb", "bulk", "bullet", "bundle", "bunker", "burden",
        "burger", "burst", "bus", "business", "busy", "butter", "buyer",
        "buzz", "cabbage", "cabin", "cable", "cactus", "cage", "cake", "call",
        "calm", "camera", "camp", "can", "canal", "cancel", "candy", "cannon",
        "canoe", "canvas", "canyon", "capable", "capital", "captain", "car",
        "carbon", "card", "cargo", "carpet", "carry", "cart", "case", "cash",
        "casino", "castle", "casual", "cat", "catalog", "catch", "category",
        "cattle", "caught", "cause", "caution", "cave", "ceiling", "celery",
        "cement", "census", "century", "cereal", "certain", "chair", "chalk",
        "champion", "change", "chaos", "chapter", "charge", "chase", "chat",
        "cheap", "check", "cheese", "chef", "cherry", "chest", "chicken",
        "chief", "child", "chimney", "choice", "choose", "chronic", "chuckle",
        "chunk", "churn", "cigar", "cinnamon", "circle", "citizen", "city",
        "civil", "claim", "clap", "clarify", "claw", "clay", "clean", "clerk",
        "clever", "click", "client", "cliff", "climb", "clinic", "clip",
        "clock", "clog", "close", "cloth", "cloud", "clown", "club", "clump",
        "cluster", "clutch", "coach", "coast", "coconut", "code", "coffee",
        "coil", "coin", "collect", "color", "column", "combine", "come",
        "comfort", "comic", "common", "company", "concert", "conduct",
        "confirm", "congress", "connect", "consider", "control", "convince",
        "cook", "cool", "copper", "copy", "coral", "core", "corn", "correct",
        "cost", "cotton", "couch", "country", "couple", "course", "cousin",
        "cover", "coyote", "crack", "cradle", "craft", "cram", "crane",
        "crash", "crater", "crawl", "crazy", "cream", "credit", "creek",
        "crew", "cricket", "crime", "crisp", "critic", "crop", "cross",
        "crouch", "crowd", "crucial", "cruel", "cruise", "crumble", "crunch",
        "crush", "cry", "crystal", "cube", "culture", "cup", "cupboard",
        "curious", "current", "curtain", "curve", "cushion", "custom", "cute",
        "cycle", "dad", "damage", "damp", "dance", "danger", "daring", "dash",
        "daughter", "dawn", "day", "deal", "debate", "debris", "decade",
        "december", "decide", "decline", "decorate", "decrease", "deer",
        "defense", "define", "defy", "degree", "delay", "deliver", "demand",
        "demise", "denial", "dentist", "deny", "depart", "depend", "deposit",
        "depth", "deputy", "derive", "describe", "desert", "design", "desk",
        "despair", "destroy", "detail", "detect", "develop", "device",
        "devote", "diagram", "dial", "diamond", "diary", "dice", "diesel",
        "diet", "differ", "digital", "dignity", "dilemma", "dinner",
        "dinosaur", "direct", "dirt", "disagree", "discover", "disease",
        "dish", "dismiss", "disorder", "display", "distance", "divert",
        "divide", "divorce", "dizzy", "doctor", "document", "dog", "doll",
        "dolphin", "domain", "donate", "donkey", "donor", "door", "dose",
        "double", "dove", "draft", "dragon", "drama", "drastic", "draw",
        "dream", "dress", "drift", "drill", "drink", "drip", "drive", "drop",
        "drum", "dry", "duck", "dumb", "dune", "during", "dust", "dutch",
        "duty", "dwarf", "dynamic", "eager", "eagle", "early", "earn",
        "earth", "easily", "east", "easy", "echo", "ecology", "economy",
        "edge", "edit", "educate", "effort", "egg", "eight", "either",
        "elbow", "elder", "electric", "elegant", "element", "elephant",
        "elevator", "elite", "else", "embark", "embody", "embrace", "emerge",
        "emotion", "employ", "empower", "empty", "enable", "enact", "end",
        "endless", "endorse", "enemy", "energy", "enforce", "engage",
        "engine", "enhance", "enjoy", "enlist", "enough", "enrich", "enroll",
        "ensure", "enter", "entire", "entry", "envelope", "episode", "equal",
        "equip", "era", "erase", "erode", "erosion", "error", "erupt",
        "escape", "essay", "essence", "estate", "eternal", "ethics",
        "evidence", "evil", "evoke", "evolve", "exact", "example", "excess",
        "exchange", "excite", "exclude", "excuse", "execute", "exercise",
        "exhaust", "exhibit", "exile", "exist", "exit", "exotic", "expand",
        "expect", "expire", "explain", "expose", "express", "extend", "extra",
        "eye", "eyebrow", "fabric", "face", "faculty", "fade", "faint",
        "faith", "fall", "false", "fame", "family", "famous", "fan", "fancy",
        "fantasy", "farm", "fashion", "fat", "fatal", "father", "fatigue",
        "fault", "favorite", "feature", "february", "federal", "fee", "feed",
        "feel", "female", "fence", "festival", "fetch", "fever", "few",
        "fiber", "fiction", "field", "figure", "file", "film", "filter",
        "final", "find", "fine", "finger", "finish", "fire", "firm", "first",
        "fiscal", "fish", "fit", "fitness", "fix", "flag", "flame", "flash",
        "flat", "flavor", "flee", "flight", "flip", "float", "flock", "floor",
        "flower", "fluid", "flush", "fly", "foam", "focus", "fog", "foil",
        "fold", "follow", "food", "foot", "force", "forest", "forget", "fork",
        "fortune", "forum", "forward", "fossil", "foster", "found", "fox",
        "fragile", "frame", "frequent", "fresh", "friend", "fringe", "frog",
        "front", "frost", "frown", "frozen", "fruit", "fuel", "fun", "funny",
        "furnace", "fury", "future", "gadget", "gain", "galaxy", "gallery",
        "game", "gap", "garage", "garbage", "garden", "garlic", "garment",
        "gas", "gasp", "gate", "gather", "gauge", "gaze", "general", "genius",
        "genre", "gentle", "genuine", "gesture", "ghost", "giant", "gift",
        "giggle", "ginger", "giraffe", "girl", "give", "glad", "glance",
        "glare", "glass", "glide", "glimpse", "globe", "gloom", "glory",
        "glove", "glow", "glue", "goat", "goddess", "gold", "good", "goose",
        "gorilla", "gospel", "gossip", "govern", "gown", "grab", "grace",
        "grain", "grant", "grape", "grass", "gravity", "great", "green",
        "grid", "grief", "grit", "grocery", "group", "grow", "grunt", "guard",
        "guess", "guide", "guilt", "guitar", "gun", "gym", "habit", "hair",
        "half", "hammer", "hamster", "hand", "happy", "harbor", "hard",
        "harsh", "harvest", "hat", "have", "hawk", "hazard", "head", "health",
        "heart", "heavy", "hedgehog", "height", "hello", "helmet", "help",
        "hen", "hero", "hidden", "high", "hill", "hint", "hip", "hire",
        "history", "hobby", "hockey", "hold", "hole", "holiday", "hollow",
        "home", "honey", "hood", "hope", "horn", "horror", "horse",
        "hospital", "host", "hotel", "hour", "hover", "hub", "huge", "human",
        "humble", "humor", "hundred", "hungry", "hunt", "hurdle", "hurry",
        "hurt", "husband", "hybrid", "ice", "icon", "idea", "identify",
        "idle", "ignore", "ill", "illegal", "illness", "image", "imitate",
        "immense", "immune", "impact", "impose", "improve", "impulse", "inch",
        "include", "income", "increase", "index", "indicate", "indoor",
        "industry", "infant", "inflict", "inform", "inhale", "inherit",
        "initial", "inject", "injury", "inmate", "inner", "innocent", "input",
        "inquiry", "insane", "insect", "inside", "inspire", "install",
        "intact", "interest", "into", "invest", "invite", "involve", "iron",
        "island", "isolate", "issue", "item", "ivory", "jacket", "jaguar",
        "jar", "jazz", "jealous", "jeans", "jelly", "jewel", "job", "join",
        "joke", "journey", "joy", "judge", "juice", "jump", "jungle",
        "junior", "junk", "just", "kangaroo", "keen", "keep", "ketchup",
        "key", "kick", "kid", "kidney", "kind", "kingdom", "kiss", "kit",
        "kitchen", "kite", "kitten", "kiwi", "knee", "knife", "knock", "know",
        "lab", "label", "labor", "ladder", "lady", "lake", "lamp", "language",
        "laptop", "large", "later", "latin", "laugh", "laundry", "lava",
        "law", "lawn", "lawsuit", "layer", "lazy", "leader", "leaf", "learn",
        "leave", "lecture", "left", "leg", "legal", "legend", "leisure",
        "lemon", "lend", "length", "lens", "leopard", "lesson", "letter",
        "level", "liar", "liberty", "library", "license", "life", "lift",
        "light", "like", "limb", "limit", "link", "lion", "liquid", "list",
        "little", "live", "lizard", "load", "loan", "lobster", "local",
        "lock", "logic", "lonely", "long", "loop", "lottery", "loud",
        "lounge", "love", "loyal", "lucky", "luggage", "lumber", "lunar",
        "lunch", "luxury", "lyrics", "machine", "mad", "magic", "magnet",
        "maid", "mail", "main", "major", "make", "mammal", "man", "manage",
        "mandate", "mango", "mansion", "manual", "maple", "marble", "march",
        "margin", "marine", "market", "marriage", "mask", "mass", "master",
        "match", "material", "math", "matrix", "matter", "maximum", "maze",
        "meadow", "mean", "measure", "meat", "mechanic", "medal", "media",
        "melody", "melt", "member", "memory", "mention", "menu", "mercy",
        "merge", "merit", "merry", "mesh", "message", "metal", "method",
        "middle", "midnight", "milk", "million", "mimic", "mind", "minimum",
        "minor", "minute", "miracle", "mirror", "misery", "miss", "mistake",
        "mix", "mixed", "mixture", "mobile", "model", "modify", "mom",
        "moment", "monitor", "monkey", "monster", "month", "moon", "moral",
        "more", "morning", "mosquito", "mother", "motion", "motor",
        "mountain", "mouse", "move", "movie", "much", "muffin", "mule",
        "multiply", "muscle", "museum", "mushroom", "music", "must", "mutual",
        "myself", "mystery", "myth", "naive", "name", "napkin", "narrow",
        "nasty", "nation", "nature", "near", "neck", "need", "negative",
        "neglect", "neither", "nephew", "nerve", "nest", "net", "network",
        "neutral", "never", "news", "next", "nice", "night", "noble", "noise",
        "nominee", "noodle", "normal", "north", "nose", "notable", "note",
        "nothing", "notice", "novel", "now", "nuclear", "number", "nurse",
        "nut", "oak", "obey", "object", "oblige", "obscure", "observe",
        "obtain", "obvious", "occur", "ocean", "october", "odor", "off",
        "offer", "office", "often", "oil", "okay", "old", "olive", "olympic",
        "omit", "once", "one", "onion", "online", "only", "open", "opera",
        "opinion", "oppose", "option", "orange", "orbit", "orchard", "order",
        "ordinary", "organ", "orient", "original", "orphan", "ostrich",
        "other", "outdoor", "outer", "output", "outside", "oval", "oven",
        "over", "own", "owner", "oxygen", "oyster", "ozone", "pact", "paddle",
        "page", "pair", "palace", "palm", "panda", "panel", "panic",
        "panther", "paper", "parade", "parent", "park", "parrot", "party",
        "pass", "patch", "path", "patient", "patrol", "pattern", "pause",
        "pave", "payment", "peace", "peanut", "pear", "peasant", "pelican",
        "pen", "penalty", "pencil", "people", "pepper", "perfect", "permit",
        "person", "pet", "phone", "photo", "phrase", "physical", "piano",
        "picnic", "picture", "piece", "pig", "pigeon", "pill", "pilot",
        "pink", "pioneer", "pipe", "pistol", "pitch", "pizza", "place",
        "planet", "plastic", "plate", "play", "please", "pledge", "pluck",
        "plug", "plunge", "poem", "poet", "point", "polar", "pole", "police",
        "pond", "pony", "pool", "popular", "portion", "position", "possible",
        "post", "potato", "pottery", "poverty", "powder", "power", "practice",
        "praise", "predict", "prefer", "prepare", "present", "pretty",
        "prevent", "price", "pride", "primary", "print", "priority", "prison",
        "private", "prize", "problem", "process", "produce", "profit",
        "program", "project", "promote", "proof", "property", "prosper",
        "protect", "proud", "provide", "public", "pudding", "pull", "pulp",
        "pulse", "pumpkin", "punch", "pupil", "puppy", "purchase", "purity",
        "purpose", "purse", "push", "put", "puzzle", "pyramid", "quality",
        "quantum", "quarter", "question", "quick", "quit", "quiz", "quote",
        "rabbit", "raccoon", "race", "rack", "radar", "radio", "rail", "rain",
        "raise", "rally", "ramp", "ranch", "random", "range", "rapid", "rare",
        "rate", "rather", "raven", "raw", "razor", "ready", "real", "reason",
        "rebel", "rebuild", "recall", "receive", "recipe", "record",
        "recycle", "reduce", "reflect", "reform", "refuse", "region",
        "regret", "regular", "reject", "relax", "release", "relief", "rely",
        "remain", "remember", "remind", "remove", "render", "renew", "rent",
        "reopen", "repair", "repeat", "replace", "report", "require",
        "rescue", "resemble", "resist", "resource", "response", "result",
        "retire", "retreat", "return", "reunion", "reveal", "review",
        "reward", "rhythm", "rib", "ribbon", "rice", "rich", "ride", "ridge",
        "rifle", "right", "rigid", "ring", "riot", "ripple", "risk", "ritual",
        "rival", "river", "road", "roast", "robot", "robust", "rocket",
        "romance", "roof", "rookie", "room", "rose", "rotate", "rough",
        "round", "route", "royal", "rubber", "rude", "rug", "rule", "run",
        "runway", "rural", "sad", "saddle", "sadness", "safe", "sail",
        "salad", "salmon", "salon", "salt", "salute", "same", "sample",
        "sand", "satisfy", "satoshi", "sauce", "sausage", "save", "say",
        "scale", "scan", "scare", "scatter", "scene", "scheme", "school",
        "science", "scissors", "scorpion", "scout", "scrap", "screen",
        "script", "scrub", "sea", "search", "season", "seat", "second",
        "secret", "section", "security", "seed", "seek", "segment", "select",
        "sell", "seminar", "senior", "sense", "sentence", "series", "service",
        "session", "settle", "setup", "seven", "shadow", "shaft", "shallow",
        "share", "shed", "shell", "sheriff", "shield", "shift", "shine",
        "ship", "shiver", "shock", "shoe", "shoot", "shop", "short",
        "shoulder", "shove", "shrimp", "shrug", "shuffle", "shy", "sibling",
        "sick", "side", "siege", "sight", "sign", "silent", "silk", "silly",
        "silver", "similar", "simple", "since", "sing", "siren", "sister",
        "situate", "six", "size", "skate", "sketch", "ski", "skill", "skin",
        "skirt", "skull", "slab", "slam", "sleep", "slender", "slice",
        "slide", "slight", "slim", "slogan", "slot", "slow", "slush", "small",
        "smart", "smile", "smoke", "smooth", "snack", "snake", "snap",
        "sniff", "snow", "soap", "soccer", "social", "sock", "soda", "soft",
        "solar", "soldier", "solid", "solution", "solve", "someone", "song",
        "soon", "sorry", "sort", "soul", "sound", "soup", "source", "south",
        "space", "spare", "spatial", "spawn", "speak", "special", "speed",
        "spell", "spend", "sphere", "spice", "spider", "spike", "spin",
        "spirit", "split", "spoil", "sponsor", "spoon", "sport", "spot",
        "spray", "spread", "spring", "spy", "square", "squeeze", "squirrel",
        "stable", "stadium", "staff", "stage", "stairs", "stamp", "stand",
        "start", "state", "stay", "steak", "steel", "stem", "step", "stereo",
        "stick", "still", "sting", "stock", "stomach", "stone", "stool",
        "story", "stove", "strategy", "street", "strike", "strong",
        "struggle", "student", "stuff", "stumble", "style", "subject",
        "submit", "subway", "success", "such", "sudden", "suffer", "sugar",
        "suggest", "suit", "summer", "sun", "sunny", "sunset", "super",
        "supply", "supreme", "sure", "surface", "surge", "surprise",
        "surround", "survey", "suspect", "sustain", "swallow", "swamp",
        "swap", "swarm", "swear", "sweet", "swift", "swim", "swing", "switch",
        "sword", "symbol", "symptom", "syrup", "system", "table", "tackle",
        "tag", "tail", "talent", "talk", "tank", "tape", "target", "task",
        "taste", "tattoo", "taxi", "teach", "team", "tell", "ten", "tenant",
        "tennis", "tent", "term", "test", "text", "thank", "that", "theme",
        "then", "theory", "there", "they", "thing", "this", "thought",
        "three", "thrive", "throw", "thumb", "thunder", "ticket", "tide",
        "tiger", "tilt", "timber", "time", "tiny", "tip", "tired", "tissue",
        "title", "toast", "tobacco", "today", "toddler", "toe", "together",
        "toilet", "token", "tomato", "tomorrow", "tone", "tongue", "tonight",
        "tool", "tooth", "top", "topic", "topple", "torch", "tornado",
        "tortoise", "toss", "total", "tourist", "toward", "tower", "town",
        "toy", "track", "trade", "traffic", "tragic", "train", "transfer",
        "trap", "trash", "travel", "tray", "treat", "tree", "trend", "trial",
        "tribe", "trick", "trigger", "trim", "trip", "trophy", "trouble",
        "truck", "true", "truly", "trumpet", "trust", "truth", "try", "tube",
        "tuition", "tumble", "tuna", "tunnel", "turkey", "turn", "turtle",
        "twelve", "twenty", "twice", "twin", "twist", "two", "type",
        "typical", "ugly", "umbrella", "unable", "unaware", "uncle",
        "uncover", "under", "undo", "unfair", "unfold", "unhappy", "uniform",
        "unique", "unit", "universe", "unknown", "unlock", "until", "unusual",
        "unveil", "update", "upgrade", "uphold", "upon", "upper", "upset",
        "urban", "urge", "usage", "use", "used", "useful", "useless", "usual",
        "utility", "vacant", "vacuum", "vague", "valid", "valley", "valve",
        "van", "vanish", "vapor", "various", "vast", "vault", "vehicle",
        "velvet", "vendor", "venture", "venue", "verb", "verify", "version",
        "very", "vessel", "veteran", "viable", "vibrant", "vicious",
        "victory", "video", "view", "village", "vintage", "violin", "virtual",
        "virus", "visa", "visit", "visual", "vital", "vivid", "vocal",
        "voice", "void", "volcano", "volume", "vote", "voyage", "wage",
        "wagon", "wait", "walk", "wall", "walnut", "want", "warfare", "warm",
        "warrior", "wash", "wasp", "waste", "water", "wave", "way", "wealth",
        "weapon", "wear", "weasel", "weather", "web", "wedding", "weekend",
        "weird", "welcome", "west", "wet", "whale", "what", "wheat", "wheel",
        "when", "where", "whip", "whisper", "wide", "width", "wife", "wild",
        "will", "win", "window", "wine", "wing", "wink", "winner", "winter",
        "wire", "wisdom", "wise", "wish", "witness", "wolf", "woman",
        "wonder", "wood", "wool", "word", "work", "world", "worry", "worth",
        "wrap", "wreck", "wrestle", "wrist", "write", "wrong", "yard", "year",
        "yellow", "you", "young", "youth", "zebra", "zero", "zone", "zoo",
    ]
    wordlist = phrase.rstrip().split()
    if len(wordlist) not in [12, 15, 18, 21, 24]:
        raise ValueError("BIP-39 phrase has incorrect length")
    indices = []
    for word in wordlist:
        try:
            index = BIP39_TABLE_EN.index(word)
        except ValueError:
            raise ValueError("{} is not a BIP-39 word".format(word))
        indices.append(index)

    data = bytearray()
    bucket = 0
    bits_in_bucket = 0
    for index in indices:
        bucket = (bucket << 11) | index
        bits_in_bucket += 11
        while bits_in_bucket >= 8:
            data.append((bucket >> (bits_in_bucket - 8)) & 0xFF)
            mask = 0xFFFF_FFFF ^ (0xFF << (bits_in_bucket - 8))
            bucket &= mask
            bits_in_bucket -= 8

    if bits_in_bucket == 0:
        entered_checksum = data[-1]
        data = data[:-1]
    else:
        entered_checksum = bucket

    hasher = SHA256.new()
    hasher.update(data)
    digest = hasher.digest()
    checksum_bits = len(data) // 4
    checksum = digest[0] >> (8 - checksum_bits)

    if checksum == entered_checksum:
        return data
    else:
        raise ValueError("checksum did not match on BIP-39 phrase")

class PrecursorUsb:
    def __init__(self, dev):
        self.dev = dev
        self.RDSR = 0x05
        self.RDSCUR = 0x2B
        self.RDID = 0x9F
        self.WREN = 0x06
        self.WRDI = 0x04
        self.SE4B = 0x21
        self.BE4B = 0xDC
        self.PP4B = 0x12
        self.registers = {}
        self.regions = {}
        self.gitrev = ''

    def register(self, name):
        return int(self.registers[name], 0)

    def halt(self):
        if 'vexriscv_debug' in self.regions:
            self.poke(int(self.regions['vexriscv_debug'][0], 0), 0x00020000)
        elif 'reboot_cpu_hold_reset' in self.registers:
            self.poke(self.register('reboot_cpu_hold_reset'), 1)
        else:
            print("Can't find reset CSR. Try updating to the latest version of this program")

    def unhalt(self):
        if 'vexriscv_debug' in self.regions:
            self.poke(int(self.regions['vexriscv_debug'][0], 0), 0x02000000)
        elif 'reboot_cpu_hold_reset' in self.registers:
            self.poke(self.register('reboot_cpu_hold_reset'), 0)
        else:
            print("Can't find reset CSR. Try updating to the latest version of this program")

    def peek(self, addr, display=False):
        _dummy_s = '\x00'.encode('utf-8')
        data = array.array('B', _dummy_s * 4)

        numread = self.dev.ctrl_transfer(bmRequestType=(0x80 | 0x43), bRequest=0,
        wValue=(addr & 0xffff), wIndex=((addr >> 16) & 0xffff),
        data_or_wLength=data, timeout=500)

        read_data = int.from_bytes(data.tobytes(), byteorder='little', signed=False)
        if display == True:
            print("0x{:08x}".format(read_data))
        return read_data

    def poke(self, addr, wdata, check=False, display=False):
        if check == True:
            _dummy_s = '\x00'.encode('utf-8')
            data = array.array('B', _dummy_s * 4)

            numread = self.dev.ctrl_transfer(bmRequestType=(0x80 | 0x43), bRequest=0,
            wValue=(addr & 0xffff), wIndex=((addr >> 16) & 0xffff),
            data_or_wLength=data, timeout=500)

            read_data = int.from_bytes(data.tobytes(), byteorder='little', signed=False)
            print("before poke: 0x{:08x}".format(read_data))

        data = array.array('B', wdata.to_bytes(4, 'little'))
        numwritten = self.dev.ctrl_transfer(bmRequestType=(0x00 | 0x43), bRequest=0,
            wValue=(addr & 0xffff), wIndex=((addr >> 16) & 0xffff),
            data_or_wLength=data, timeout=500)

        if check == True:
            _dummy_s = '\x00'.encode('utf-8')
            data = array.array('B', _dummy_s * 4)

            numread = self.dev.ctrl_transfer(bmRequestType=(0x80 | 0x43), bRequest=0,
            wValue=(addr & 0xffff), wIndex=((addr >> 16) & 0xffff),
            data_or_wLength=data, timeout=500)

            read_data = int.from_bytes(data.tobytes(), byteorder='little', signed=False)
            print("after poke: 0x{:08x}".format(read_data))
        if display == True:
            print("wrote 0x{:08x} to 0x{:08x}".format(wdata, addr))

    def burst_read(self, addr, len):
        _dummy_s = '\x00'.encode('utf-8')
        maxlen = 4096

        ret = bytearray()
        packet_count = len // maxlen
        if (len % maxlen) != 0:
            packet_count += 1

        for pkt_num in range(packet_count):
            cur_addr = addr + pkt_num * maxlen
            if pkt_num == packet_count - 1:
                if len % maxlen != 0:
                    bufsize = len % maxlen
                else:
                    bufsize = maxlen
            else:
                bufsize = maxlen

            data = array.array('B', _dummy_s * bufsize)
            numread = self.dev.ctrl_transfer(bmRequestType=(0x80 | 0x43), bRequest=0,
                wValue=(cur_addr & 0xffff), wIndex=((cur_addr >> 16) & 0xffff),
                data_or_wLength=data, timeout=500)

            if numread != bufsize:
                print("Burst read error: {} bytes requested, {} bytes read at 0x{:08x}".format(bufsize, numread, cur_addr))
                exit(1)

            ret = ret + data

        return ret

    def burst_write(self, addr, data):
        if len(data) == 0:
            return

        # the actual "addr" doesn't matter for a burst_write, because it's specified
        # as an argument to the flash_pp4b command. We lock out access to the base of
        # SPINOR because it's part of the gateware, so, we pick a "safe" address to
        # write to instead. The page write responder will aggregate any write data
        # to anywhere in the SPINOR address range.
        writebuf_addr = 0x2098_0000 # the current start address of the kernel, for example

        maxlen = 4096
        packet_count = len(data) // maxlen
        if (len(data) % maxlen) != 0:
            packet_count += 1

        for pkt_num in range(packet_count):
            cur_addr = addr + pkt_num * maxlen
            if pkt_num == packet_count - 1:
                if len(data) % maxlen != 0:
                    bufsize = len(data) % maxlen
                else:
                    bufsize = maxlen
            else:
                bufsize = maxlen

            wdata = array.array('B', data[(pkt_num * maxlen):(pkt_num * maxlen) + bufsize])
            numwritten = self.dev.ctrl_transfer(bmRequestType=(0x00 | 0x43), bRequest=0,
                # note use of writebuf_addr instead of cur_addr -> see comment above about the quirk of write addressing
                wValue=(writebuf_addr & 0xffff), wIndex=((writebuf_addr >> 16) & 0xffff),
                data_or_wLength=wdata, timeout=500)

            if numwritten != bufsize:
                print("Burst write error: {} bytes requested, {} bytes written at 0x{:08x}".format(bufsize, numwritten, cur_addr))
                exit(1)

    def ping_wdt(self):
        self.poke(self.register('wdt_watchdog'), 1, display=False)
        self.poke(self.register('wdt_watchdog'), 1, display=False)

    def spinor_command_value(self, exec=0, lock_reads=0, cmd_code=0, dummy_cycles=0, data_words=0, has_arg=0):
        return ((exec & 1) << 1 |
                (lock_reads & 1) << 24 |
                (cmd_code & 0xff) << 2 |
                (dummy_cycles & 0x1f) << 11 |
                (data_words & 0xff) << 16 |
                (has_arg & 1) << 10
               )

    def flash_rdsr(self, lock_reads):
        self.poke(self.register('spinor_cmd_arg'), 0)
        self.poke(self.register('spinor_command'),
            self.spinor_command_value(exec=1, lock_reads=lock_reads, cmd_code=self.RDSR, dummy_cycles=4, data_words=1, has_arg=1)
        )
        return self.peek(self.register('spinor_cmd_rbk_data'), display=False)

    def flash_rdscur(self):
        self.poke(self.register('spinor_cmd_arg'), 0)
        self.poke(self.register('spinor_command'),
            self.spinor_command_value(exec=1, lock_reads=1, cmd_code=self.RDSCUR, dummy_cycles=4, data_words=1, has_arg=1)
        )
        return self.peek(self.register('spinor_cmd_rbk_data'), display=False)

    def flash_rdid(self, offset):
        self.poke(self.register('spinor_cmd_arg'), 0)
        self.poke(self.register('spinor_command'),
            self.spinor_command_value(exec=1, cmd_code=self.RDID, dummy_cycles=4, data_words=offset, has_arg=1)
        )
        return self.peek(self.register('spinor_cmd_rbk_data'), display=False)

    def flash_wren(self):
        self.poke(self.register('spinor_cmd_arg'), 0)
        self.poke(self.register('spinor_command'),
            self.spinor_command_value(exec=1, lock_reads=1, cmd_code=self.WREN)
        )

    def flash_wrdi(self):
        self.poke(self.register('spinor_cmd_arg'), 0)
        self.poke(self.register('spinor_command'),
            self.spinor_command_value(exec=1, lock_reads=1, cmd_code=self.WRDI)
        )

    def flash_se4b(self, sector_address):
        self.poke(self.register('spinor_cmd_arg'), sector_address)
        self.poke(self.register('spinor_command'),
            self.spinor_command_value(exec=1, lock_reads=1, cmd_code=self.SE4B, has_arg=1)
        )

    def flash_be4b(self, block_address):
        self.poke(self.register('spinor_cmd_arg'), block_address)
        self.poke(self.register('spinor_command'),
            self.spinor_command_value(exec=1, lock_reads=1, cmd_code=self.BE4B, has_arg=1)
        )

    def flash_pp4b(self, address, data_bytes):
        self.poke(self.register('spinor_cmd_arg'), address)
        self.poke(self.register('spinor_command'),
            self.spinor_command_value(exec=1, lock_reads=1, cmd_code=self.PP4B, has_arg=1, data_words=(data_bytes//2))
        )

    def load_csrs(self, fname=None):
        LOC_CSRCSV = 0x20277000 # this address shouldn't change because it's how we figure out our version number
        # CSR extraction:
        # dd if=soc_csr.bin of=csr_data_0.9.6.bin skip=2524 count=32 bs=1024
        if fname == None:
            csr_data = self.burst_read(LOC_CSRCSV, 0x8000)
        else:
            with open(fname, "rb") as f:
                csr_data = f.read(0x8000)

        hasher = hashlib.sha512()
        hasher.update(csr_data[:0x7FC0])
        digest = hasher.digest()
        if digest != csr_data[0x7fc0:]:
            print("Could not find a valid csr.csv descriptor on the device, aborting!")
            exit(1)

        csr_len = int.from_bytes(csr_data[:4], 'little')
        csr_extracted = csr_data[4:4+csr_len]
        decoded = csr_extracted.decode('utf-8')
        # strip comments
        stripped = []
        for line in decoded.split('\n'):
            if line.startswith('#') == False:
                stripped.append(line)
        # create database
        csr_db = csv.reader(stripped)
        for row in csr_db:
            if len(row) > 1:
                if 'csr_register' in row[0]:
                    self.registers[row[1]] = row[2]
                if 'memory_region' in row[0]:
                    self.regions[row[1]] = [row[2], row[3]]
                if 'git_rev' in row[0]:
                    self.gitrev = row[1]
        print("Using SoC {} registers".format(self.gitrev))

    def erase_region(self, addr, length):
        # ID code check
        code = self.flash_rdid(1)
        print("ID code bytes 1-2: 0x{:08x}".format(code))
        if code != 0x8080c2c2:
            print("ID code mismatch")
            exit(1)
        code = self.flash_rdid(2)
        print("ID code bytes 2-3: 0x{:08x}".format(code))
        if code != 0x3b3b8080:
            print("ID code mismatch")
            exit(1)

        # block erase
        progress = ProgressBar(min_value=0, max_value=length, prefix='Erasing ').start()
        erased = 0
        while erased < length:
            self.ping_wdt()
            if (length - erased >= 65536) and ((addr & 0xFFFF) == 0):
                blocksize = 65536
            else:
                blocksize = 4096

            while True:
                self.flash_wren()
                status = self.flash_rdsr(1)
                if status & 0x02 != 0:
                    break

            if blocksize == 4096:
                self.flash_se4b(addr + erased)
            else:
                self.flash_be4b(addr + erased)
            erased += blocksize

            while (self.flash_rdsr(1) & 0x01) != 0:
                pass

            result = self.flash_rdscur()
            if result & 0x60 != 0:
                print("E_FAIL/P_FAIL set on erase, programming may fail, but trying anyways...")

            if self.flash_rdsr(1) & 0x02 != 0:
                self.flash_wrdi()
                while (self.flash_rdsr(1) & 0x02) != 0:
                    pass
            if erased < length:
                progress.update(erased)
        progress.finish()
        print("Erase finished")

    # addr is relative to the base of FLASH (not absolute)
    def flash_program(self, addr, data, verify=True):
        flash_region = int(self.regions['spiflash'][0], 0)
        flash_len = int(self.regions['spiflash'][1], 0)

        if (addr + len(data) > flash_len):
            print("Write data out of bounds! Aborting.")
            exit(1)

        # ID code check
        code = self.flash_rdid(1)
        print("ID code bytes 1-2: 0x{:08x}".format(code))
        if code != 0x8080c2c2:
            print("ID code mismatch")
            exit(1)
        code = self.flash_rdid(2)
        print("ID code bytes 2-3: 0x{:08x}".format(code))
        if code != 0x3b3b8080:
            print("ID code mismatch")
            exit(1)

        # block erase
        progress = ProgressBar(min_value=0, max_value=len(data), prefix='Erasing ').start()
        erased = 0
        while erased < len(data):
            self.ping_wdt()
            if (len(data) - erased >= 65536) and ((addr & 0xFFFF) == 0):
                blocksize = 65536
            else:
                blocksize = 4096

            while True:
                self.flash_wren()
                status = self.flash_rdsr(1)
                if status & 0x02 != 0:
                    break

            if blocksize == 4096:
                self.flash_se4b(addr + erased)
            else:
                self.flash_be4b(addr + erased)
            erased += blocksize

            while (self.flash_rdsr(1) & 0x01) != 0:
                pass

            result = self.flash_rdscur()
            if result & 0x60 != 0:
                print("E_FAIL/P_FAIL set on erase, programming may fail, but trying anyways...")

            if self.flash_rdsr(1) & 0x02 != 0:
                self.flash_wrdi()
                while (self.flash_rdsr(1) & 0x02) != 0:
                    pass
            if erased < len(data):
                progress.update(erased)
        progress.finish()
        print("Erase finished")

        # program
        # pad out to the nearest word length
        if len(data) % 4 != 0:
            data += bytearray([0xff] * (4 - (len(data) % 4)))
        written = 0
        progress = ProgressBar(min_value=0, max_value=len(data), prefix='Writing ').start()
        while written < len(data):
            self.ping_wdt()
            if len(data) - written > 256:
                chunklen = 256
            else:
                chunklen = len(data) - written

            while True:
                self.flash_wren()
                status = self.flash_rdsr(1)
                if status & 0x02 != 0:
                    break

            self.burst_write(self.register('spinor_wdata'), data[written:(written+chunklen)])
            self.flash_pp4b(addr + written, chunklen)

            written += chunklen
            if written < len(data):
                progress.update(written)
        progress.finish()
        print("Write finished")

        if self.flash_rdsr(1) & 0x02 != 0:
            self.flash_wrdi()
            while (self.flash_rdsr(1) & 0x02) != 0:
                pass

        # dummy reads to clear the "read lock" bit
        self.flash_rdsr(0)

        # verify
        self.ping_wdt()
        if verify:
            print("Performing readback for verification...")
            self.ping_wdt()
            rbk_data = self.burst_read(addr + flash_region, len(data))
            if rbk_data != data:
                errs = 0
                err_thresh = 64
                for i in range(0, len(rbk_data)):
                    if rbk_data[i] != data[i]:
                        if errs < err_thresh:
                            print("Error at 0x{:x}: {:x}->{:x}".format(i, data[i], rbk_data[i]))
                        errs += 1
                    if errs == err_thresh:
                        print("Too many errors, stopping print...")
                print("Errors were found in verification, programming failed")
                print("Total byte errors: {}".format(errs))
                exit(1)
            else:
                print("Verification passed.")
        else:
            print("Skipped verification at user request")

        self.ping_wdt()

def bitflip(data_block, bitwidth=32):
    if bitwidth == 0:
        return data_block
    bytewidth = bitwidth // 8
    bitswapped = bytearray()
    i = 0
    while i < len(data_block):
        data = int.from_bytes(data_block[i:i+bytewidth], byteorder='big', signed=False)
        b = '{:0{width}b}'.format(data, width=bitwidth)
        bitswapped.extend(int(b[::-1], 2).to_bytes(bytewidth, byteorder='big'))
        i = i + bytewidth
    return bytes(bitswapped)

# assumes a, b are the same length eh?
def xor_bytes(a, b):
    i = 0
    y = bytearray()
    while i < len(a):
        y.extend((a[i] ^ b[i]).to_bytes(1, byteorder='little'))
        i = i + 1

    return bytes(y)

def try_key_to_bytes(input):
    if len(input.split(' ')) == 24: # 24 words is BIP-39
        key_bytes = bip39_to_bits(input)
    else:
        key_bytes = int(input, 16).to_bytes(32, byteorder='big')
    return key_bytes

# binfile should be the input SoC file, already read in as bytes()
# returns the encrypted version of binfile
def encrypt_to_efuse(binfile, key):
    print("Encrypting gateware to target-specific key...")
    # extract the keys
    key_bytes = bytes([0] * 32)
    new_key = try_key_to_bytes(key)
    new_hmac = get_random_bytes(32)
    new_iv = get_random_bytes(16)

    # search for structure
    # 0x3001_6004 -> specifies the CBC key
    # 4 words of CBC IV
    # 0x3003_4001 -> ciphertext len
    # 1 word of ciphertext len
    # then ciphertext

    position = 0
    iv_pos = 0
    while position < len(binfile):
        cwd = int.from_bytes(binfile[position:position+4], 'big')
        if cwd == 0x3001_6004:
            iv_pos = position+4
        if cwd == 0x3003_4001:
            break
        position = position + 1

    position = position + 4

    ciphertext_len = 4* int.from_bytes(binfile[position:position+4], 'big')
    position = position + 4

    active_area = binfile[position : position+ciphertext_len]
    postamble = binfile[position+ciphertext_len:]

    iv_bytes = bitflip(binfile[iv_pos : iv_pos+0x10])  # note that the IV is embedded in the file

    cipher = AES.new(key_bytes, AES.MODE_CBC, iv_bytes)
    plain_bitstream = cipher.decrypt(bitflip(active_area))

    # now construct the output file and its hashes
    plaintext = bytearray()
    f = bytearray()

    # fixed header that sets 66MHz config speed, x1, 1.8V, eFuse target
    device_header = [
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        0xaa, 0x99, 0x55, 0x66, 0x20, 0x00, 0x00, 0x00, 0x30, 0x03, 0xe0, 0x01, 0x00, 0x00, 0x00, 0x0b,
        0x30, 0x00, 0x80, 0x01, 0x00, 0x00, 0x00, 0x12, 0x20, 0x00, 0x00, 0x00, 0x30, 0x00, 0xc0, 0x01,
        0x80, 0x00, 0x00, 0x40, 0x30, 0x00, 0xa0, 0x01, 0x80, 0x00, 0x00, 0x40, 0x30, 0x01, 0xc0, 0x01,
        0x00, 0x00, 0x00, 0x00, 0x20, 0x00, 0x00, 0x00, 0x20, 0x00, 0x00, 0x00, 0x20, 0x00, 0x00, 0x00,
        0x20, 0x00, 0x00, 0x00, 0x20, 0x00, 0x00, 0x00, 0x20, 0x00, 0x00, 0x00, 0x20, 0x00, 0x00, 0x00,
        0x20, 0x00, 0x00, 0x00, 0x20, 0x00, 0x00, 0x00, 0x20, 0x00, 0x00, 0x00, 0x20, 0x00, 0x00, 0x00,
        0x20, 0x00, 0x00, 0x00, 0x20, 0x00, 0x00, 0x00, 0x20, 0x00, 0x00, 0x00, 0x30, 0x01, 0x60, 0x04,
    ]

    for item in device_header:  # add the cleartext header
        f.extend(bytes([item]))

    f.extend(bitflip(new_iv)) # insert the IV

    ciphertext_header = [
        0x30, 0x03, 0x40, 0x01, 0x00, 0x08, 0x5b, 0x98,
    ]
    for item in ciphertext_header:  # add the cleartext length-of-ciphertext field before the ciphertext
        f.extend(bytes([item]))

    # generate the header and footer hash keys.
    header = int(0x6C6C6C6C6C6C6C6C6C6C6C6C6C6C6C6C6C6C6C6C6C6C6C6C6C6C6C6C6C6C6C6C).to_bytes(32, byteorder='big')
    keyed_header = xor_bytes(header, new_hmac)
    footer = int(0x3A3A3A3A3A3A3A3A3A3A3A3A3A3A3A3A3A3A3A3A3A3A3A3A3A3A3A3A3A3A3A3A).to_bytes(32, byteorder='big')
    keyed_footer = xor_bytes(footer, new_hmac)

    # add the header
    plaintext.extend(keyed_header)
    plaintext.extend(header)

    # insert the bitstream plaintext, skipping the header and the trailing HMAC.
    plaintext.extend(plain_bitstream[64:-160])

    # compute first HMAC of stream with new HMAC key
    h1 = SHA256.new()
    k = 0
    while k < len(plaintext) - 320:  # HMAC does /not/ cover the whole file, it stops 320 bytes short of the end
        h1.update(bitflip(plaintext[k:k+16], 32))
        k = k + 16
    h1_digest = h1.digest()

    # add the footer
    plaintext.extend(keyed_footer)
    plaintext.extend(footer)
    plaintext.extend(bytes(32)) # empty spot where hash #1 would be stored
    hash_pad = [ # sha-256 padding for the zero'd hash #1, which is in the bitstream and seems necessary for verification
        0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xc0, 0x00, 0x00,
    ]
    plaintext.extend(hash_pad)

    # compute the hash of the hash, presumably to prevent length extension attacks?
    h2 = SHA256.new()
    h2.update(bitflip(keyed_footer))
    h2.update(bitflip(footer))
    h2.update(h1_digest)
    h2_digest = h2.digest()

    # commit the final HMAC to the bitstream plaintext
    plaintext.extend(bitflip(h2_digest))

    # encrypt the bitstream
    newcipher = AES.new(new_key, AES.MODE_CBC, new_iv)

    # finally generate the ciphertext block, which encapsulates the HMACs
    ciphertext = newcipher.encrypt(bytes(plaintext))

    # add ciphertext to the bitstream
    f.extend(bitflip(ciphertext))

    # add the cleartext postamble to the bitstream. These are a series of NOP commands + all of the csr.csv data & signatures
    f.extend(postamble)
    print("Encryption success! {} bytes generated.".format(len(f)))
    assert len(f) == 2621440, "Encryption length is incorrect; aborting!"

    return f

def auto_int(x):
    return int(x, 0)

def main():
    parser = argparse.ArgumentParser(description="Developer tool for loading local images to a Precursor. Use `precursorupdater` to fetch from a pre-built image.")
    parser.add_argument(
        "--soc", required=False, help="'Factory Reset' the SoC gateware. Note: this will overwrite any secret keys stored in your device!", type=str, nargs='?', metavar=('SoC gateware file'), const='../precursors/soc_csr.bin'
    )
    parser.add_argument(
        "-s", "--staging", required=False, help="Stage an update to apply", type=str, nargs='?', metavar=('SoC gateware file'), const='../precursors/soc_csr.bin'
    )
    parser.add_argument(
        "-l", "--loader", required=False, help="Loader", type=str, nargs='?', metavar=('loader file'), const='../target/riscv32imac-unknown-xous-elf/release/loader.bin'
    )
    parser.add_argument(
        "--disable-boot", required=False, action='store_true', help="Disable system boot (for use in multi-stage updates)"
    )
    parser.add_argument(
        "--enable-boot-wipe", required=False, action='store_true', help="Re-enable system boot for factory reset. Requires both a loader (-l) and a soc (--soc) spec. Overwrites root keys."
    )
    parser.add_argument(
        "--enable-boot-update", required=False, action='store_true', help="Re-enable system boot for updates. Requires both a loader (-l) and a staging (-s) spec. Stages SOC without overwriting root keys."
    )
    parser.add_argument(
        "-k", "--kernel", required=False, help="Kernel", type=str, nargs='?', metavar=('kernel file'), const='../target/riscv32imac-unknown-xous-elf/release/xous.img'
    )
    parser.add_argument(
        "-e", "--ec", required=False, help="EC gateware", type=str, nargs='?', metavar=('EC gateware package'), const='ec_fw.bin'
    )
    parser.add_argument(
        "-w", "--wf200", required=False, help="WF200 firmware", type=str, nargs='?', metavar=('WF200 firmware package'), const='wf200_fw.bin'
    )
    parser.add_argument(
        "--erase-pddb", help="Erase the PDDB area", action="store_true"
    )
    parser.add_argument(
        "--audiotest", required=False, help="Test audio clip (must be 8kHz WAV)", type=str, nargs='?', metavar=('Test audio clip'), const="testaudio.wav"
    )
    parser.add_argument(
        "--peek", required=False, help="Inspect an address", type=auto_int, metavar=('ADDR')
    )
    parser.add_argument(
        "--poke", required=False, help="Write to an address", type=auto_int, nargs=2, metavar=('ADDR', 'DATA')
    )
    parser.add_argument(
        "--check-poke", required=False, action='store_true', help="Read data before and after the poke"
    )
    parser.add_argument(
        "--config", required=False, help="Print the descriptor", action='store_true'
    )
    parser.add_argument(
        "-i", "--image", required=False, help="Manually specify an image and address. Offset is relative to bottom of flash.", type=str, nargs=2, metavar=('IMAGEFILE', 'ADDR')
    )
    parser.add_argument(
        "--verify", help="Readback verification. May fail for large files due to WDT timeout.", default=False, action='store_true'
    )
    parser.add_argument(
        "--force", help="Ignore gitrev version on SoC and try to burn an image anyways", action="store_true"
    )
    parser.add_argument(
        "--bounce", help="cycle the device through a reset", action="store_true"
    )
    parser.add_argument(
        "--factory-new", help="reset the entire image to mimic exactly what comes out of the factory, including temp files for testing. Warning: this will take a long time.", action="store_true"
    )
    parser.add_argument(
        "--override-csr", required=False, help="CSR file to use instead of CSR values stored with the image. Used to recover in case of partial update of soc_csr.bin", type=str,
    )
    parser.add_argument(
        "--dump", required=False, help="Dump a region of memory. Takes a virtual memory map file as an argument. Looks for lines of format `V|P vaddr paddr`", type=str,
    )
    parser.add_argument(
        "--dump-file", required=False, help="Name out output file for dump. Required if --dump is specified.", type=str,
    )
    parser.add_argument(
        "--key", help="Backup key in hex or BIP-39 format. Used to factory-reset efused devices. Specify BIP-39 phrase within double-quotes.", type=str
    )
    args = parser.parse_args()

    if not len(sys.argv) > 1:
        print("No arguments specified, doing nothing. Use --help for more information.")
        print("If you are looking to fetch pre-built images and burn them, this is the wrong tool:")
        print("Use `precursorupdater` instead (via `pip3 install precursorupdater`)")
        exit(1)

    dev = usb.core.find(idProduct=0x5bf0, idVendor=0x1209)

    if dev is None:
        raise ValueError('Precursor device not found')

    dev.set_configuration()
    if args.config:
        cfg = dev.get_active_configuration()
        print(cfg)

    pc_usb = PrecursorUsb(dev)

    if args.verify:
        verify = True
    else:
        verify = False

    if args.peek:
        pc_usb.peek(args.peek, display=True)
        # print(burst_read(dev, args.peek, 256).hex())
        exit(0)

    if args.poke:
        addr, data = args.poke
        pc_usb.poke(addr, data, check=args.check_poke, display=True)
        # import os
        # d = bytearray(os.urandom(8000))
        # burst_write(dev, addr, d)
        # r = burst_read(dev, addr, 8000)
        # print(r.hex())
        # if d != r:
        #     print("mismatch")
        # else:
        #     print("match")
        exit(0)

    pc_usb.load_csrs(args.override_csr) # prime the CSR values
    if "v0.8" in pc_usb.gitrev:
        locs = {
           "LOC_SOC"    : [0x0000_0000, "soc_csr.bin"],
           "LOC_STAGING": [0x0028_0000, "pass"],
           "LOC_LOADER" : [0x0050_0000, "loader.bin"],
           "LOC_KERNEL" : [0x0098_0000, "xous.img"],
           "LOC_WF200"  : [0x07F8_0000, "pass"],
           "LOC_EC"     : [0x07FC_E000, "pass"],
           "LOC_AUDIO"  : [0x0634_0000, "short_8khz.wav"],
           "LEN_AUDIO"  : [0x01C4_0000, "pass"],
           "LOC_PDDB"   : [0x0100_0000, "pass"],
        }
    elif "v0.9" in pc_usb.gitrev:
        locs = {
            "LOC_SOC"    : [0x0000_0000, "soc_csr.bin"],
            "LOC_STAGING": [0x0028_0000, "pass"],
            "LOC_LOADER" : [0x0050_0000, "loader.bin"],
            "LOC_KERNEL" : [0x0098_0000, "xous.img"],
            "LOC_WF200"  : [0x07F8_0000, "pass"],
            "LOC_EC"     : [0x07FC_E000, "pass"],
            "LOC_AUDIO"  : [0x0634_0000, "short_8khz.wav"],
            "LEN_AUDIO"  : [0x01C4_0000, "pass"],
            "LOC_PDDB"   : [0x01D8_0000, "pass"],
        }
    elif args.force == True:
        # try the v0.9 offsets
        locs = {
           "LOC_SOC"    : [0x00000000, "soc_csr.bin"],
           "LOC_STAGING": [0x00280000, "pass"],
           "LOC_LOADER" : [0x00500000, "loader.bin"],
           "LOC_KERNEL" : [0x00980000, "xous.img"],
           "LOC_WF200"  : [0x07F80000, "pass"],
           "LOC_EC"     : [0x07FCE000, "pass"],
           "LOC_AUDIO"  : [0x06340000, "short_8khz.wav"],
           "LEN_AUDIO"  : [0x01C40000, "pass"],
           "LOC_PDDB"   : [0x01D80000, "pass"],
        }
    else:
        print("SoC is from an unknown rev '{}', use --force to continue anyways with v0.9 firmware offsets".format(pc_usb.load_csrs()))
        exit(1)

    pc_usb.ping_wdt()

    if args.dump:
        table = []
        with open(args.dump, 'r') as map:
            for line in map.readlines():
                rgx = re.search('.*V\|P\s([0-9a-fA-F]*)\s([0-9a-fA-F]*).*', line)
                if rgx is not None:
                    map = rgx.groups()
                    table += [int(map[1], 16)]

        if args.dump_file is None:
            print("Must specify --dump-file when using --dump")
            exit(0)
        with open(args.dump_file, 'wb') as f:
            for page in table:
                pc_usb.halt()
                data = pc_usb.burst_read(page, 4096)
                pc_usb.unhalt()
                f.write(data)
        exit(0)

    print("Halting CPU.")
    pc_usb.halt()

    if args.erase_pddb:
        print("Erasing PDDB region")
        pc_usb.erase_region(locs['LOC_PDDB'][0], locs['LOC_EC'][0] - locs['LOC_PDDB'][0])

    if args.disable_boot:
        print("Disabling boot")
        pc_usb.erase_region(locs['LOC_LOADER'][0], 1024 * 256)

    if args.enable_boot_wipe:
        if args.loader == None:
            print("Must provide both a loader and soc image")
        if args.soc == None:
            print("Must provide both a soc and loader image")
        print("Enabling boot with {} and {}".format(args.loader, args.soc))
        print("WARNING: if a backup key is set, the correct key MUST be specified with `--key`, or else the device will be bricked.")
        print("Continue? (y/n)")
        confirm = input()
        if len(confirm) > 0 and confirm.lower()[:1] == 'y':
            print("Programming loader image {}".format(args.loader))
            with open(args.loader, "rb") as f:
                image = f.read()
                pc_usb.flash_program(locs['LOC_LOADER'][0], image, verify=verify)
            print("Programming SoC gateware".format(args.soc))
            with open(args.soc, "rb") as f:
                image = f.read()
                if args.key is not None:
                    image = encrypt_to_efuse(image, args.key)
                if verify == True:
                    print("Note: SoC verification is not possible as readback is locked for security purposes")
                pc_usb.flash_program(locs['LOC_SOC'][0], image, verify=False)

            print("Erasing PDDB root structures")
            pc_usb.erase_region(locs['LOC_PDDB'][0], 1024 * 1024)

            print("Resuming CPU.")
            pc_usb.unhalt()

            print("Resetting SOC...")
            try:
                pc_usb.poke(pc_usb.register('reboot_soc_reset'), 0xac, display=False)
            except usb.core.USBError:
                pass # we expect an error because we reset the SOC and that includes the USB core
            exit(0)

    if args.enable_boot_update:
        if args.loader == None:
            print("Must provide both a loader and soc image")
        if args.staging == None:
            print("Must provide both a soc and loader image")
        print("Enabling boot with {} and {}".format(args.loader, args.staging))
        print("Programming loader image {}".format(args.loader))
        with open(args.loader, "rb") as f:
            image = f.read()
            pc_usb.flash_program(locs['LOC_LOADER'][0], image, verify=verify)
        print("Staging SoC gateware".format(args.staging))
        with open(args.staging, "rb") as f:
            image = f.read()
            if verify == True:
                print("Note: staging area verification is not possible as readback is locked for security purposes")
            pc_usb.flash_program(locs['LOC_STAGING'][0], image, verify=verify)

        print("Resuming CPU.")
        pc_usb.unhalt()

        print("Resetting SOC...")
        try:
            pc_usb.poke(pc_usb.register('reboot_soc_reset'), 0xac, display=False)
        except usb.core.USBError:
            pass # we expect an error because we reset the SOC and that includes the USB core
        exit(0)

    if args.image:
        image_file, addr_str = args.image
        addr = int(addr_str, 0)
        print("Burning manually specified image '{}' to address 0x{:08x} relative to bottom of FLASH".format(image_file, addr))
        with open(image_file, "rb") as f:
            image_data = f.read()
            pc_usb.flash_program(addr, image_data, verify=verify)

    if args.ec != None:
        print("Staging EC firmware package '{}' in SOC memory space...".format(args.ec))
        with open(args.ec, "rb") as f:
            image = f.read()
            pc_usb.flash_program(locs['LOC_EC'][0], image, verify=verify)

    if args.wf200 != None:
        print("Staging WF200 firmware package '{}' in SOC memory space...".format(args.wf200))
        with open(args.wf200, "rb") as f:
            image = f.read()
            pc_usb.flash_program(locs['LOC_WF200'][0], image, verify=verify)

    if args.staging != None:
        print("Staging SoC gateware {}".format(args.soc))
        with open(args.staging, "rb") as f:
            image = f.read()
            if verify == True:
                print("Note: staging area verification is not possible as readback is locked for security purposes")
            pc_usb.flash_program(locs['LOC_STAGING'][0], image, verify=verify)

    if args.kernel != None:
        print("Programming kernel image {}".format(args.kernel))
        with open(args.kernel, "rb") as f:
            image = f.read()
            pc_usb.flash_program(locs['LOC_KERNEL'][0], image, verify=verify)

    if args.loader != None:
        print("Programming loader image {}".format(args.loader))
        with open(args.loader, "rb") as f:
            image = f.read()
            pc_usb.flash_program(locs['LOC_LOADER'][0], image, verify=verify)

    if args.soc != None:
        if args.force == True:
            print("`--force` specified; all checks disabled, you may brick your device!")
            print("Programming SoC gateware {}".format(args.soc))
            with open(args.soc, "rb") as f:
                image = f.read()
                if verify == True:
                    print("Note: SoC verification is not possible as readback is locked for security purposes")
                pc_usb.flash_program(locs['LOC_SOC'][0], image, verify=False)
                print("Erasing PDDB root structures")
                pc_usb.erase_region(locs['LOC_PDDB'][0], 1024 * 1024)
        else:
            print("This will overwrite any secret keys in your device and erase PDDB keys.")
            print("WARNING: if a backup key is set, the correct key MUST be specified with `--key`, or else the device will be bricked.")
            print("Continue? (y/n)")
            confirm = input()
            if len(confirm) > 0 and confirm.lower()[:1] == 'y':
                print("Programming SoC gateware {}".format(args.soc))
                with open(args.soc, "rb") as f:
                    image = f.read()
                    if args.key is not None:
                        image = encrypt_to_efuse(image, args.key)
                    if verify == True:
                        print("Note: SoC verification is not possible as readback is locked for security purposes")
                    pc_usb.flash_program(locs['LOC_SOC'][0], image, verify=False)
                    print("Erasing PDDB root structures")
                    pc_usb.erase_region(locs['LOC_PDDB'][0], 1024 * 1024)


    if args.audiotest != None:
        print("Loading audio test clip {}".format(args.audiotest))
        with open(args.audiotest, "rb") as f:
            image = f.read()
            if len(image) >= locs['LEN_AUDIO'][0]:
                print("audio file is too long, aborting audio burn!")
            else:
                pc_usb.flash_program(locs['LOC_AUDIO'][0], image, verify=verify)

    if args.factory_new:
        print("WARNING: if a backup key is set, the correct key MUST be specified with `--key`, or else the device will be bricked.")
        print("Continue? (y/n)")
        confirm = input()
        if len(confirm) > 0 and confirm.lower()[:1] == 'y':
            base_url = "https://ci.betrusted.io/releases/v0.9.5/"
            # erase the entire flash
            pc_usb.erase_region(0, 0x800_0000)
            # burn the gateware
            for sections in locs.values():
                if sections[1] != 'pass':
                    print('retrieving {}'.format(base_url + sections[1]))
                    with urllib.request.urlopen(base_url + sections[1]) as f:
                        print('burning at {:x}'.format(sections[0]))
                        image = f.read()
                        if sections[1] == "soc_csr.bin" and args.key is not None:
                            image = encrypt_to_efuse(image, args.key)
                        pc_usb.flash_program(sections[0], image, verify=False)


    print("Resuming CPU.")
    pc_usb.unhalt()

    print("Resetting SOC...")
    try:
        pc_usb.poke(pc_usb.register('reboot_soc_reset'), 0xac, display=False)
    except usb.core.USBError:
        pass # we expect an error because we reset the SOC and that includes the USB core

    # print("If you need to run more commands, please unplug and re-plug your device in, as the Precursor USB core was just reset")

if __name__ == "__main__":
    main()
    exit(0)
