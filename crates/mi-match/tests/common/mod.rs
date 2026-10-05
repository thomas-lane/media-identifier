//! A fictional show and a speech-recognition error simulator for matching tests.
//!
//! The show, "Grandpa Owl's Fables", is ten short episodes written for these tests (retellings of
//! public-domain Aesop fables). Every episode opens and closes with the same theme lines, like a
//! real show, and some share characters (two mouse stories), so the tests also cover shared
//! text. Results on this fixture say nothing about accuracy on real recordings.

#![allow(dead_code)]

use mi_match::{EpisodeInput, FileInput};
use mi_types::{
    Episode, EpisodeKey, EpisodeOrdering, FileId, ProviderId, ReferenceText, ShowRef, TextKind,
};

pub const THEME_OPEN: &str = "Gather round, little ones, it's story time with Grandpa Owl!\n\
    Hoo hoo, who wants a story tonight?\n";
pub const THEME_CLOSE: &str = "\nAnd that's the end of tonight's tale.\n\
    Goodnight, sleep tight, from Grandpa Owl.";

pub const TITLES: [&str; 10] = [
    "The Tortoise and the Hare",
    "The Boy Who Cried Wolf",
    "The Ant and the Grasshopper",
    "The Lion and the Mouse",
    "The Fox and the Grapes",
    "The Town Mouse and the Country Mouse",
    "The Goose That Laid the Golden Eggs",
    "The North Wind and the Sun",
    "The Crow and the Pitcher",
    "The Dog and His Reflection",
];

pub const BODIES: [&str; 10] = [
    // 1
    "Once there was a hare who bragged to everybody in the meadow.\n\
     I am the fastest animal alive, nobody can beat me in a race!\n\
     The tortoise looked up from his lettuce and said, I will race you.\n\
     You? The hare laughed so hard he fell over. You are the slowest creature I know.\n\
     Slow and steady, said the tortoise. Shall we start at the old oak tree?\n\
     The fox agreed to be the judge, and he waved a red handkerchief.\n\
     Off went the hare like a rocket, leaving a cloud of dust behind him.\n\
     Halfway along the path he looked back and could not even see the tortoise.\n\
     I have plenty of time for a nap, he yawned, and he lay down under a hedge.\n\
     The tortoise plodded past the hedge, one foot after another, never stopping.\n\
     When the hare woke up the sun was setting and he sprinted for the finish line.\n\
     But the tortoise was already there, and all the animals were cheering.\n\
     Slow and steady wins the race, said the tortoise with a smile.",
    // 2
    "A shepherd boy watched the sheep on the hillside above the village.\n\
     It was a quiet, boring job, and he wanted some excitement.\n\
     So he filled his lungs and shouted, Wolf! Wolf! A wolf is chasing the sheep!\n\
     The villagers dropped their shovels and buckets and ran up the hill.\n\
     Where is the wolf, gasped the baker, holding a rolling pin.\n\
     The boy giggled. There is no wolf, I was only joking.\n\
     The villagers grumbled and walked back down to their work.\n\
     The next day he did it again, and again they came running, and again there was no wolf.\n\
     Then one evening a real wolf crept out of the forest, with yellow eyes and sharp teeth.\n\
     Wolf! Wolf! Please help me, cried the boy, but nobody came.\n\
     They thought he was playing his trick again.\n\
     The wolf scattered the whole flock across the valley.\n\
     Nobody believes a liar, even when he is telling the truth.",
    // 3
    "All summer long the ants carried seeds and grain to their nest under the stone.\n\
     The grasshopper lay in the warm grass playing his fiddle.\n\
     Why are you working so hard on such a lovely day, he asked.\n\
     We are storing food for the winter, said the ant, wiping her brow.\n\
     Winter is months away, laughed the grasshopper. Come and dance with me instead.\n\
     The ant shook her head and lifted another heavy kernel of corn.\n\
     The grasshopper played jigs and reels until the leaves turned orange.\n\
     Then the snow came, deep and cold, and there was nothing left to eat.\n\
     Shivering and starving, the grasshopper knocked on the door of the ant hill.\n\
     Please, may I have a little corn? I am so hungry.\n\
     What were you doing all summer, asked the ant.\n\
     I was making music, said the grasshopper sadly.\n\
     Then you may dance the winter away, said the ant, but she shared a little soup.\n\
     It is wise to prepare today for the needs of tomorrow.",
    // 4
    "A mighty lion was sleeping in the jungle when a tiny mouse ran across his nose.\n\
     The lion woke with a roar and trapped the mouse beneath his enormous paw.\n\
     Please let me go, squeaked the mouse. One day I might be able to help you.\n\
     The lion chuckled. How could a little thing like you ever help the king of beasts?\n\
     But he was amused, and he lifted his paw and let the mouse scamper away.\n\
     A few weeks later hunters stretched a rope net between two trees.\n\
     The lion walked straight into it and the ropes tangled around his mane and legs.\n\
     He roared and struggled, but the net only grew tighter.\n\
     The mouse heard the roaring and came running through the tall grass.\n\
     She gnawed and nibbled at the thick ropes with her sharp little teeth.\n\
     Snap, snap, snap, and the net fell apart, and the lion was free.\n\
     You laughed at me, said the mouse, but now you know that even a mouse can help a lion.\n\
     No act of kindness, however small, is ever wasted.",
    // 5
    "On a hot afternoon a hungry fox wandered into a vineyard.\n\
     High above his head hung a bunch of plump purple grapes, shining in the sunshine.\n\
     Those grapes look juicy and sweet, said the fox, licking his lips.\n\
     He crouched low and jumped as high as he could, but his jaws snapped at empty air.\n\
     He backed up, took a running start and leapt again. Still too short.\n\
     A blackbird on the trellis whistled. You will never reach them, mister fox.\n\
     Mind your own business, snapped the fox, and he jumped a third time and a fourth.\n\
     At last, panting and dusty, he sat down and stared at the grapes.\n\
     Then he turned up his nose and walked away with his tail in the air.\n\
     I did not want them anyway, he muttered. I am sure those grapes are sour.\n\
     The blackbird laughed and laughed.\n\
     It is easy to sneer at what you cannot have.",
    // 6
    "A country mouse lived in a burrow at the edge of a wheat field.\n\
     One day her cousin, the town mouse, came to visit in a smart little coat.\n\
     The country mouse served barley, acorns and a dried pea for dinner.\n\
     Is this all you eat, asked the town mouse, wrinkling her whiskers.\n\
     Come to the city with me and I will show you how to live.\n\
     So they travelled to a grand house with velvet curtains and a chandelier.\n\
     On the dining table there were cakes, cheese, jelly and honey.\n\
     Just as the country mouse took her first bite of cheese, the door burst open.\n\
     A cook with a broom and a big ginger cat chased them under the cupboard.\n\
     They hid there trembling until the house was dark and still.\n\
     Thank you, cousin, whispered the country mouse, but I am going home tomorrow.\n\
     I would rather eat beans in peace than feast in fear.\n\
     A simple life with safety is better than luxury with danger.",
    // 7
    "A poor farmer and his wife went out one morning to feed their goose.\n\
     In the straw of her nest they found an egg that glittered like the sun.\n\
     It is made of solid gold, gasped the wife, weighing it in her hands.\n\
     The next morning there was another golden egg, and the morning after that, another.\n\
     They sold the eggs at the market and bought a new barn and a silver carriage.\n\
     But the richer they grew, the greedier they became.\n\
     One egg a day is far too slow, grumbled the farmer.\n\
     The goose must be full of gold inside. Let us take it all at once.\n\
     His wife fetched the carving knife from the kitchen.\n\
     But when they opened the goose there was no gold inside at all.\n\
     And now there would never be another golden egg.\n\
     The farmer sat down in the empty barn and wept.\n\
     Greed can destroy the very thing that makes you rich.",
    // 8
    "High in the sky the north wind and the sun were having an argument.\n\
     I am the strongest force in the whole world, howled the north wind.\n\
     I am not so sure, said the sun gently. Let us have a contest.\n\
     Do you see that traveller walking down the road in a woollen cloak?\n\
     Whoever can make him take off his cloak shall be the strongest.\n\
     The north wind went first. He blew a freezing gale that bent the trees.\n\
     He whipped up hail and sleet, but the traveller only pulled his cloak tighter.\n\
     The harder the wind blew, the tighter the man held on.\n\
     At last the wind gave up, exhausted and out of breath.\n\
     Then the sun came out from behind a cloud and shone warmly on the road.\n\
     The traveller wiped his forehead, unbuttoned his cloak and laid it on a rock.\n\
     You see, said the sun, warmth works where force fails.\n\
     Gentle persuasion is stronger than bluster.",
    // 9
    "It had not rained for weeks and every pond and puddle had dried up.\n\
     A thirsty crow flew from field to field looking for something to drink.\n\
     At last she spotted a tall clay pitcher standing in a garden.\n\
     She hopped onto the rim and peered inside. There was water at the very bottom.\n\
     She pushed her beak down as far as it would go, but she could not reach it.\n\
     She tried to tip the pitcher over, but it was far too heavy.\n\
     Caw, caw, I will die of thirst, she croaked.\n\
     Then she noticed some pebbles scattered on the garden path.\n\
     She picked up a pebble in her beak and dropped it into the pitcher. Plink.\n\
     Then another pebble, and another, plink, plink, plink.\n\
     Slowly the water rose higher and higher toward the top.\n\
     At last she could reach it, and she drank and drank until she was full.\n\
     Little by little does the trick, and necessity is the mother of invention.",
    // 10
    "A dog had stolen a juicy bone from the butcher's shop and trotted off with it.\n\
     He was heading home to eat it in peace under the porch.\n\
     On the way he crossed a narrow wooden bridge over a stream.\n\
     He looked down into the calm water and saw another dog looking up at him.\n\
     And that dog was holding an even bigger bone in its mouth.\n\
     I want that bone too, thought the greedy dog, and he growled at the stranger.\n\
     The other dog growled right back.\n\
     So the greedy dog opened his mouth wide to bark and snatch the bigger bone.\n\
     Splash! His own bone fell into the stream and sank to the bottom.\n\
     The ripples spread, and the other dog vanished, because it was only his reflection.\n\
     The dog stood on the bridge with nothing at all, and slunk home hungry.\n\
     If you grasp at the shadow you may lose the substance.",
];

/// A bonus feature: talks about the show, mentions some of its characters, but is no episode.
pub const FEATURETTE: &str = "Hi, I'm the producer, and welcome to our studio.\n\
    Today we're going behind the scenes to see how an episode gets made.\n\
    First the writers sit around this big table with coffee and argue about jokes.\n\
    Then our voice actors record their lines in this little booth with foam on the walls.\n\
    Our Grandpa Owl actor has been with us since the very first season.\n\
    The animators draw thousands of frames on their tablets, and the computers colour them in.\n\
    Each half hour takes about nine months from the first script to the final mix.\n\
    The composer records the music with a small orchestra on Tuesday mornings.\n\
    Our favourite part is the test screening, when real kids watch and tell us what they think.\n\
    Thanks for visiting, and keep watching for more bonus features on this disc.";

pub fn show() -> ShowRef {
    ShowRef {
        provider: ProviderId::Tvmaze,
        id: "999".into(),
    }
}

pub fn episode_text(i: usize) -> String {
    format!("{THEME_OPEN}{}{THEME_CLOSE}", BODIES[i])
}

pub fn reference(i: usize, kind: TextKind, text: String) -> ReferenceText {
    ReferenceText {
        show_ref: show(),
        ordering: EpisodeOrdering::Aired,
        episode: EpisodeKey {
            season: 1,
            number: i as u32 + 1,
        },
        kind,
        provider: ProviderId::Subdl,
        provider_ref: format!("sub-{i}"),
        text,
        language: "en".into(),
        fetched_at_ms: 0,
    }
}

/// Listed runtime of every episode, seconds.
pub const RUNTIME_S: f64 = 600.0;

pub fn episode(i: usize) -> Episode {
    Episode {
        show_ref: show(),
        ordering: EpisodeOrdering::Aired,
        key: EpisodeKey {
            season: 1,
            number: i as u32 + 1,
        },
        title: TITLES[i].into(),
        runtime_s: Some(RUNTIME_S),
        airdate: None,
        summary: None,
        provider_episode_id: format!("ep{i}"),
    }
}

/// All ten episodes with their subtitles as reference text.
pub fn episodes_with_subtitles() -> Vec<EpisodeInput> {
    (0..10)
        .map(|i| EpisodeInput {
            episode: episode(i),
            texts: vec![reference(i, TextKind::Subtitles, episode_text(i))],
        })
        .collect()
}

pub fn file(name: &str, transcript: String, duration_s: f64) -> FileInput {
    FileInput {
        file_id: FileId(name.into()),
        duration_s,
        transcript,
        embedded_text: None,
        mostly_music: false,
        play_all_position: None,
        sampled_windows: None,
    }
}

/// A small deterministic pseudo-random generator (xorshift64*).
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    pub fn chance(&mut self, p: f64) -> bool {
        ((self.next_u64() >> 11) as f64 / (1u64 << 53) as f64) < p
    }

    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
}

/// How badly the simulated speech recogniser mishears.
#[derive(Debug, Clone, Copy)]
pub struct Errors {
    /// Chance that a word with a common homophone is written as the homophone.
    pub homophone: f64,
    /// Chance that a word is misspelt (a letter dropped, doubled, swapped or changed).
    pub misspell: f64,
    /// Chance that a word is replaced by an unrelated word.
    pub substitute: f64,
    /// Chance that a word is dropped.
    pub drop: f64,
    /// Chance that a filler word is inserted before a word.
    pub insert: f64,
}

impl Errors {
    /// Roughly a 25-30% word error rate.
    pub const MODERATE: Errors = Errors {
        homophone: 0.6,
        misspell: 0.08,
        substitute: 0.08,
        drop: 0.08,
        insert: 0.04,
    };
    /// Roughly a 45-50% word error rate, as for sung or noisy audio.
    pub const HEAVY: Errors = Errors {
        homophone: 0.8,
        misspell: 0.15,
        substitute: 0.15,
        drop: 0.15,
        insert: 0.08,
    };
}

const HOMOPHONES: [(&str, &str); 24] = [
    ("there", "their"),
    ("their", "there"),
    ("to", "two"),
    ("too", "to"),
    ("for", "four"),
    ("hare", "hair"),
    ("tale", "tail"),
    ("knew", "new"),
    ("sun", "son"),
    ("piece", "peace"),
    ("would", "wood"),
    ("sea", "see"),
    ("right", "write"),
    ("night", "knight"),
    ("tonight", "to night"),
    ("everybody", "every body"),
    ("nobody", "no body"),
    ("one", "won"),
    ("eight", "ate"),
    ("whole", "hole"),
    ("by", "buy"),
    ("tied", "tide"),
    ("mouse", "mice"),
    ("cannot", "can not"),
];

const RANDOM_WORDS: [&str; 16] = [
    "table", "orange", "window", "seven", "basket", "yellow", "monday", "ladder", "pocket",
    "simple", "garden", "cup", "river", "paper", "silver", "button",
];

const FILLERS: [&str; 4] = ["uh", "um", "the", "and"];

/// Mishears `text` as a speech recogniser might: homophones, misspellings, unrelated words,
/// dropped and inserted words; punctuation and capitals are lost, as in raw transcripts.
pub fn mishear(text: &str, errors: Errors, seed: u64) -> String {
    let mut rng = Rng::new(seed);
    let mut out: Vec<String> = Vec::new();
    for raw in text.split_whitespace() {
        let word: String = raw
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '\'')
            .collect::<String>()
            .to_lowercase();
        if word.is_empty() {
            continue;
        }
        if rng.chance(errors.insert) {
            out.push(FILLERS[rng.below(FILLERS.len())].to_string());
        }
        if rng.chance(errors.drop) {
            continue;
        }
        if let Some((_, h)) = HOMOPHONES.iter().find(|(w, _)| *w == word)
            && rng.chance(errors.homophone)
        {
            out.push(h.to_string());
            continue;
        }
        if rng.chance(errors.substitute) {
            out.push(RANDOM_WORDS[rng.below(RANDOM_WORDS.len())].to_string());
            continue;
        }
        if word.len() > 3 && rng.chance(errors.misspell) {
            out.push(misspell(&word, &mut rng));
            continue;
        }
        out.push(word);
    }
    out.join(" ")
}

fn misspell(word: &str, rng: &mut Rng) -> String {
    let mut chars: Vec<char> = word.chars().collect();
    let i = 1 + rng.below(chars.len() - 2);
    match rng.below(4) {
        0 => {
            chars.remove(i);
        }
        1 => chars.insert(i, chars[i]),
        2 => chars.swap(i, i + 1),
        _ => {
            let vowels = ['a', 'e', 'i', 'o', 'u'];
            chars[i] = vowels[rng.below(vowels.len())];
        }
    }
    chars.into_iter().collect()
}

/// The middle `share` of a text's words, as when only a sample window was transcribed.
pub fn excerpt(text: &str, start_share: f64, share: f64) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    let start = (words.len() as f64 * start_share) as usize;
    let len = ((words.len() as f64 * share) as usize).max(1);
    words[start..(start + len).min(words.len())].join(" ")
}

/// Word error rate between a reference and a mishearing (edit distance over words / words).
pub fn word_error_rate(reference: &str, heard: &str) -> f64 {
    let norm = |t: &str| -> Vec<String> {
        t.split_whitespace()
            .map(|w| {
                w.chars()
                    .filter(|c| c.is_alphanumeric())
                    .collect::<String>()
                    .to_lowercase()
            })
            .filter(|w| !w.is_empty())
            .collect()
    };
    let (a, b) = (norm(reference), norm(heard));
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        prev = cur;
    }
    prev[b.len()] as f64 / a.len().max(1) as f64
}
