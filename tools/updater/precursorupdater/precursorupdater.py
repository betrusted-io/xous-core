#! /usr/bin/env python3

import argparse
import usb.core
import usb.util
from datetime import datetime
from progressbar.bar import ProgressBar
import requests

try:
    from .precursorusb import PrecursorUsb
except ImportError:
    from precursorusb import PrecursorUsb

from Crypto.Cipher import AES
from Crypto.Hash import SHA256
from Crypto.Random import get_random_bytes

QR_CODE = """

  ██████████████  ██  ████  ██████    ██      ██████████████
  ██          ██      ██████    ██  ████  ██  ██          ██
  ██  ██████  ██  ████████████      ██  ████  ██  ██████  ██
  ██  ██████  ██      ████      ██  ██  ██    ██  ██████  ██
  ██  ██████  ██      ████      ████  ██      ██  ██████  ██
  ██          ██  ██████    ████  ██████      ██          ██
  ██████████████  ██  ██  ██  ██  ██  ██  ██  ██████████████
                      ██    ██  ████  ██████
  ██  ██      ████  ██████████  ████      ██    ██    ██  ██
  ██████    ██  ██    ████      ██████  ██    ████      ████
    ██  ██    ██████    ██  ████████████████  ████  ████  ██
  ████    ██    ██  ██        ██      ██  ████      ██
  ████  ████  ██████    ██████████    ██████  ████        ██
    ██  ██        ██  ██  ████████████  ██████████      ████
    ████████  ██  ████  ████    ████    ██    ████        ██
  ██████  ████    ████  ██████  ██    ██  ██    ████
    ████████████████████  ████    ██    ████  ████        ██
    ██              ████        ████  ██████████      ██████
  ████    ██  ██  ██    ██  ████████  ██████████  ████    ██
        ██            ██  ██  ██████      ██  ██  ██
  ████  ████  ████████  ████████  ██    ██████████████  ██
                  ██        ██████      ████      ██████  ██
  ██████████████  ████████      ██████    ██  ██  ██      ██
  ██          ██    ██  ██  ██    ██  ██████      ██    ████
  ██  ██████  ██      ████  ██  ██      ██████████████  ████
  ██  ██████  ██                ████  ████████    ██████  ██
  ██  ██████  ██  ████████  ██████    ██████      ██    ████
  ██          ██        ████  ██  ██  ████████  ██████
  ██████████████  ████  ██  ████████      ██  ████████    ██

"""

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

def get_with_progress(url, name='Downloading'):
    r = requests.get(url, stream=True)
    total_length = int(r.headers.get('content-length'))
    ret = bytearray()
    progress = ProgressBar(min_value=0, max_value=total_length, prefix=name + ' ').start()
    for chunk in r.iter_content(chunk_size=65536):
        if chunk:
            ret += bytearray(chunk)
            progress.update(len(ret))
    progress.finish()
    return ret

def get_usb_interface(config=False, peek=None, override_csr=None, force=False):
    try:
        dev = usb.core.find(idProduct=0x5bf0, idVendor=0x1209)
    except usb.core.NoBackendError:
        raise ValueError('No USB library backend available (eg Libusb, OpenUSB, etc).\nSee: https://github.com/pyusb/pyusb/blob/master/docs/faq.rst#how-do-i-fix-no-backend-available-errors')

    if dev is None:
        raise ValueError('Precursor device not found. Please check the USB cable and ensure that `usb debug` was run in Shellchat')

    dev.set_configuration()
    if config:
        cfg = dev.get_active_configuration()
        print(cfg)

    pc_usb = PrecursorUsb(dev)

    if peek:
        pc_usb.peek(peek, display=True)
        exit(0)

    pc_usb.load_csrs(override_csr) # prime the CSR values
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
    elif force == True:
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

    return (locs, pc_usb)

def main():
    parser = argparse.ArgumentParser(description="Precursor USB Updater v2", prog="python3 -m precursorupdater")
    parser.add_argument(
        "-b", "--bleeding-edge", required=False, help="Update to bleeding-edge CI build", action='store_true'
    )
    parser.add_argument(
        "-l", "--language", help="Select Xous language [en|ja|zh|fr|en-tts]", required=False, type=str, default="en"
    )
    parser.add_argument(
        "--factory-reset", required=False, help="Delete passwords and do a factory reset", action='store_true'
    )
    parser.add_argument(
        "--paranoid", required=False, help="Do a full-wipe of the PDDB after the factory reset", action='store_true'
    )
    parser.add_argument(
        "--config", required=False, help="Print the Precursor USB descriptor", action='store_true'
    )
    parser.add_argument(
        "--override-csr", required=False, help="CSR file to use instead of CSR values stored with the image. Used to recover in case of a partially corrupted gateware", type=str,
    )
    parser.add_argument(
        "--peek", required=False, help="Inspect an address, then quit. Useful for sanity testing your config.", type=auto_int, metavar=('ADDR')
    )
    parser.add_argument(
        "--force", required=False, help="Ignore gitrev version on SoC and try to burn an image anyways", action="store_true"
    )
    parser.add_argument(
        "--dry-run", required=False, help="Don't actually burn anything, but print what would happen if we did", action="store_true"
    )
    parser.add_argument(
        "--key", help="Backup key in hex or BIP-39 format. Used to factory-reset eFused devices. Specify BIP-39 phrase within double-quotes.", type=str
    )

    args = parser.parse_args()

    VALID_LANGUAGES = {
        "en",
        "en-tts",
        "ja",
        "zh",
        "fr",
    }
    language = args.language.lower()
    if language not in VALID_LANGUAGES:
        print("Language selection '{}' is not valid. Please select one of {}".format(language, VALID_LANGUAGES))
        exit(1)

    # initial check to see if the Precursor device is there
    try:
        (locs, pc_usb) = get_usb_interface(args.config, args.peek, args.override_csr, args.force)
    except ValueError as e:
        print(str(e))
        exit(1)

    if args.config:
        print("--config selected, quitting.")
        exit(0)

    # now try to download all the artifacts and check their versions
    # this list should visit kernels in order from newest to oldest.
    URL_STABLE = 'https://ci.betrusted.io/releases/latest/'
    URL_BLEEDING = 'https://ci.betrusted.io/latest-ci/'
    print("Phase 1: Download the update")
    if args.bleeding_edge:
        print("Bleeding edge CI build selected")
        url = URL_BLEEDING
    else:
        print("Latest stable build selected")
        url = URL_STABLE

    try:
        while True:
            # first try the stable branch and see if it meets the version requirement
            kernel = get_with_progress(url + 'xous-' + language + '.img', 'Kernel')
            if int.from_bytes(kernel[:4], 'little') != 1 and int.from_bytes(kernel[:4], 'little') != 2:
                print("Downloaded kernel image has unexpected signature version. Aborting.")
                exit(1)
            kern_len = int.from_bytes(kernel[4:8], 'little') + 0x1000
            if len(kernel) != kern_len:
                print("Downloaded kernel has the wrong length. Aborting.")
                attempt += 1
                exit(1)
            curver_loc = kern_len - 4 - 4 - 16
            curver = SemVer(kernel[curver_loc:curver_loc + 16])

            loader = get_with_progress(url + 'loader.bin', 'Loader')
            soc_csr = get_with_progress(url + 'soc_csr.bin', 'Gateware')
            ec_fw = get_with_progress(url + 'ec_fw.bin', 'Embedded Controller')
            wf200 = get_with_progress(url + 'wf200_fw.bin', 'WF200')

            print("Downloaded Xous version {}".format(curver.as_str()))
            break
    except Exception as e:
        if False == single_yes_or_no_question("Error: '{}' encountered downloading the update. Retry? ".format(e)):
            print("Abort by user request.")
            exit(0)

    if args.factory_reset:
        print("\nWARNING: if a backup key is set, the correct key MUST be specified with `--key`, or else the device will be bricked.")
        if False == single_yes_or_no_question("This will permanently erase user data on the device. Proceed? "):
            print("Abort by user request.")
            exit(0)
        worklist = [
            ['erase', "Disabling boot by erasing loader...", locs['LOC_LOADER'][0], 1024 * 256],
            ['prog', "Uploading kernel", locs['LOC_KERNEL'][0], kernel],
            ['prog', "Uploading EC", locs['LOC_EC'][0], ec_fw],
            ['prog', "Uploading wf200", locs['LOC_WF200'][0], wf200],
            ['prog', "Overwriting boot gateware", locs['LOC_SOC'][0], soc_csr],
            ['erase', "Erasing any staged gateware", locs['LOC_STAGING'][0], 0x28_0000],
        ]
        if args.paranoid:
            # erase the entire area -- about 10-15 minutes
            worklist += [['erase', "Full erase of PDDB", locs['LOC_PDDB'][0], 0x620_0000]]
            worklist += [['prog', "Restoring loader", locs['LOC_LOADER'][0], loader]]
        else:
            # just deletes the page table -- about 10 seconds
            worklist += [['erase', "Shallow-delete of PDDB", locs['LOC_PDDB'][0], 1024 * 512]]
            worklist += [['prog', "Restoring loader", locs['LOC_LOADER'][0], loader]]
    else:
        worklist = [
            ['erase', "Disabling boot by erasing loader...", locs['LOC_LOADER'][0], 1024 * 256],
            ['prog', "Uploading kernel", locs['LOC_KERNEL'][0], kernel],
            ['prog', "Uploading EC", locs['LOC_EC'][0], ec_fw],
            ['prog', "Uploading wf200", locs['LOC_WF200'][0], wf200],
            ['prog', "Staging gateware", locs['LOC_STAGING'][0], soc_csr],
            ['prog', "Restoring loader", locs['LOC_LOADER'][0], loader],
        ]

    print("\nPhase 2: Apply the update")
    print("Halting CPU for update.")
    pc_usb.ping_wdt()
    pc_usb.halt()
    for work in worklist:

        retry_usb = False
        while True:
            if retry_usb:
                print("Trying to re-acquire Precursor device...")
                try:
                    (locs, pc_usb) = get_usb_interface(args.config, args.peek, args.override_csr, args.force)
                except Exception as e:
                    if False == single_yes_or_no_question("Failed to find Precursor device. Try again? "):
                        print("Abort by user request!\n\nSystem may not be bootable, but you can retry an update as long as you do not power-off or hard-reset the device.")
                        exit(0)

                pc_usb.ping_wdt()
                pc_usb.halt()
                retry_usb = False

            try:
                print(work[1])
                if work[0] == 'erase':
                    if args.dry_run:
                        print("DRYRUN: would erase at 0x{:x}, len 0x{:x}".format(work[2], work[3]))
                    else:
                        pc_usb.erase_region(work[2], work[3])
                    break
                else:
                    if args.factory_reset and args.key is not None and work[1] == "Overwriting boot gateware":
                        # re-encrypt the gateware if we're doing a factory reset and a key was specified
                        work[3] = encrypt_to_efuse(work[3], args.key)
                    if args.dry_run:
                        print("DRYRUN: Would write at 0x{:x}".format(work[2]))
                    else:
                        pc_usb.flash_program(work[2], work[3], verify=False)
                    break
            except Exception as e:
                print("Error encountered while {}: '{}'".format(work[1], e))
                print("Try reseating the USB connection.")
                if False == single_yes_or_no_question("Try again? "):
                    print("Abort by user request!\n\nSystem may not be bootable, but you can retry an update as long as you do not power-off or hard-reset the device.")
                    exit(0)
                retry_usb = True

    print("Resuming CPU.")
    pc_usb.unhalt()

    print("Resetting SOC...")
    try:
        pc_usb.poke(pc_usb.register('reboot_soc_reset'), 0xac, display=False)
    except usb.core.USBError:
        pass # we expect an error because we reset the SOC and that includes the USB core


    print("\nUpdate finished!\n")
    print("{}\nVisit the QR code above to help locate the hole, or go to https://ci.betrusted.io/i/reset.jpg.".format(QR_CODE))
    print("Follow the on-device instructions and allow any follow-up actions to complete. Then, reset the device by inserting a paperclip to force a re-load the SoC image.")


def auto_int(x):
    return int(x, 0)

class SemVer:
    def __init__(self, b):
        self.maj = int.from_bytes(b[0:2], 'little')
        self.min = int.from_bytes(b[2:4], 'little')
        self.rev = int.from_bytes(b[4:6], 'little')
        self.extra = int.from_bytes(b[6:8], 'little')
        self.has_commit = int.from_bytes(b[12:16], 'little')
        # note: very old kernel will return a version of 0.0.0
        if self.has_commit != 0:
            self.commit = int.from_bytes(b[8:12], 'little')

    def ord(self): # returns a number that you can use to compare if versions are bigger or smaller than each other
        return self.maj << 48 | self.min << 32 | self.rev << 16 | self.extra

    def as_str(self):
        if self.has_commit == 0:
            return "v{}.{}.{}-{}".format(self.maj, self.min, self.rev, self.extra)
        else:
            return "v{}.{}.{}-{}-g{:x}".format(self.maj, self.min, self.rev, self.extra, self.commit)

def single_yes_or_no_question(question, default_no=False):
    choices = ' [y/N]: ' if default_no else ' [Y/n]: '
    default_answer = 'n' if default_no else 'y'
    reply = str(input(question + choices)).lower().strip() or default_answer
    if reply[0] == 'y':
        return True
    if reply[0] == 'n':
        return False
    else:
        return False if default_no else True

if __name__ == "__main__":
    main()
    exit(0)
