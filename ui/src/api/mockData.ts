// Sample data for the mock backend, modelled on the approved mockup (a Schoolhouse Rock! disc
// ripped with MakeMKV). Fictional results for UI development and tests; not real identification
// output, and the season/episode numbers follow the mockup rather than any provider.

import type {
  Candidate,
  Episode,
  EpisodeKey,
  EvidenceNote,
  FileMatch,
  HistoryEntry,
  MediaFile,
  QuotePart,
  RecentJob,
  ScanSummary,
  Show,
  ShowCandidate,
  Signals,
  SourceStatus,
  Verdict,
} from "../types/generated";

export const SAMPLE_FOLDER = "/Volumes/Rips/SCHOOLHOUSE_ROCK_D1";

export const SCHOOLHOUSE_ROCK: Show = {
  showRef: { provider: "tvmaze", id: "2617" },
  name: "Schoolhouse Rock!",
  year: 1973,
  kind: "Animation",
  seasonCount: 7,
  episodeCount: 64,
  url: null,
};

export const SHOW_CANDIDATES: ShowCandidate[] = [
  { show: SCHOOLHOUSE_ROCK, score: 1, guessedFromFolder: true },
  {
    show: {
      showRef: { provider: "tvmaze", id: "99001" },
      name: "Schoolhouse Rock Live!",
      year: 1993,
      kind: "Stage show",
      seasonCount: 1,
      episodeCount: 1,
      url: null,
    },
    score: 0.42,
    guessedFromFolder: false,
  },
];

function episode(season: number, number: number, title: string, runtimeS = 180): Episode {
  return {
    showRef: SCHOOLHOUSE_ROCK.showRef,
    ordering: "aired",
    key: { season, number },
    title,
    runtimeS,
    airdate: null,
    summary: null,
    providerEpisodeId: `${season}-${number}`,
  };
}

export const EPISODES: Episode[] = [
  episode(2, 1, "My Hero, Zero"),
  episode(2, 2, "Elementary, My Dear"),
  episode(2, 3, "Lucky Seven Sampson"),
  episode(2, 4, "Figure Eight"),
  episode(2, 5, "Three Is a Magic Number"),
  episode(2, 6, "The Four-Legged Zoo"),
  episode(2, 7, "Naughty Number Nine"),
  episode(4, 1, "Conjunction Junction"),
  episode(4, 2, "Unpack Your Adjectives"),
  episode(4, 3, "A Noun Is a Person, Place, or Thing"),
  episode(4, 4, "Verb: That's What's Happening"),
  episode(4, 5, "Interjections!"),
  episode(4, 6, "Lolly, Lolly, Lolly, Get Your Adverbs Here"),
];

function title(key: EpisodeKey): string {
  return EPISODES.find((e) => e.key.season === key.season && e.key.number === key.number)?.title ?? "";
}

function file(name: string, durationS: number, role: MediaFile["role"], chapters = 0): MediaFile {
  return {
    id: name,
    path: `${SAMPLE_FOLDER}/${name}`,
    fileName: name,
    sizeBytes: Math.round(durationS * 1_000_000),
    probe: {
      durationS,
      container: "matroska,webm",
      video: { width: 720, height: 480 },
      audioStreams: [
        { index: 1, codec: "ac3", channels: 2, sampleRate: 48000, language: "eng", isDefault: true },
      ],
      subtitleStreams: [],
      chapters: Array.from({ length: chapters }, (_, i) => ({
        index: i,
        startS: i * 185,
        endS: (i + 1) * 185,
        title: null,
      })),
    },
    role,
  };
}

interface SampleFile {
  name: string;
  durationS: number;
  /** Suggested episode, or null for an extra. */
  episode: EpisodeKey | null;
  verdict: Exclude<Verdict, "playAll">;
  score: number;
  margin: number;
  /** Zero-based play-all chapter. */
  chapter: number | null;
  signals: Signals;
  notes: EvidenceNote[];
  heard: QuotePart[];
  reference: QuotePart[];
  alternatives: { episode: EpisodeKey; score: number; signals: Signals }[];
}

const quote = (...parts: [string, boolean][]): QuotePart[] => parts.map(([text, matched]) => ({ text, matched }));

const SAMPLE_FILES: SampleFile[] = [
  {
    name: "title_t01.mkv", durationS: 183, episode: { season: 2, number: 1 }, verdict: "confident",
    score: 0.95, margin: 0.58, chapter: 0,
    signals: { dialogue: 0.9, titleHook: 1, duration: 0.97, discOrder: 0.96 },
    notes: [{ kind: "titleHeard" }, { kind: "discOrderAgrees", chapter: 0 }],
    heard: quote(["…", false], ["my hero, zero", true], [", such a funny little hero…", false]),
    reference: quote(["…", false], ["My hero, Zero", true], [", such a funny little hero…", false]),
    alternatives: [],
  },
  {
    name: "title_t02.mkv", durationS: 179, episode: { season: 2, number: 2 }, verdict: "confident",
    score: 0.93, margin: 0.51, chapter: 1,
    signals: { dialogue: 0.86, titleHook: 0.9, duration: 0.96, discOrder: 0.95 },
    notes: [{ kind: "titleHeard" }, { kind: "discOrderAgrees", chapter: 1 }],
    heard: quote(["…", false], ["elementary my dear", true], [", two times two is four…", false]),
    reference: quote(["…", false], ["Elementary, my dear", true], [", two times two is four…", false]),
    alternatives: [],
  },
  {
    name: "title_t03.mkv", durationS: 192, episode: { season: 4, number: 1 }, verdict: "confident",
    score: 0.97, margin: 0.62, chapter: 2,
    signals: { dialogue: 0.91, titleHook: 1, duration: 0.95, discOrder: 0.98 },
    notes: [{ kind: "titleHeard" }, { kind: "discOrderAgrees", chapter: 2 }],
    heard: quote(["", false], ["conjunction junction", true], [", what's your ", false], ["function", true]),
    reference: quote(["", false], ["Conjunction Junction", true], [", what's your ", false], ["function?", true]),
    alternatives: [],
  },
  {
    name: "title_t04.mkv", durationS: 185, episode: { season: 4, number: 2 }, verdict: "confident",
    score: 0.94, margin: 0.55, chapter: 3,
    signals: { dialogue: 0.88, titleHook: 1, duration: 0.97, discOrder: 0.97 },
    notes: [{ kind: "titleHeard" }, { kind: "discOrderAgrees", chapter: 3 }],
    heard: quote(["…", false], ["unpack your adjectives", true], ["…", false]),
    reference: quote(["…", false], ["Unpack your adjectives", true], ["…", false]),
    alternatives: [],
  },
  {
    name: "title_t05.mkv", durationS: 181, episode: { season: 2, number: 5 }, verdict: "confident",
    score: 0.96, margin: 0.6, chapter: 4,
    signals: { dialogue: 0.92, titleHook: 1, duration: 0.98, discOrder: 0.96 },
    notes: [{ kind: "titleHeard" }, { kind: "discOrderAgrees", chapter: 4 }],
    heard: quote(["…", false], ["three is a magic number", true], ["…", false]),
    reference: quote(["…", false], ["Three is a magic number", true], ["…", false]),
    alternatives: [],
  },
  {
    name: "title_t06.mkv", durationS: 188, episode: { season: 2, number: 6 }, verdict: "confident",
    score: 0.9, margin: 0.44, chapter: 5,
    signals: { dialogue: 0.81, titleHook: 0.85, duration: 0.95, discOrder: 0.94 },
    notes: [{ kind: "discOrderAgrees", chapter: 5 }],
    heard: quote(["…", false], ["four legged zoo", true], ["…", false]),
    reference: quote(["…", false], ["four-legged zoo", true], ["…", false]),
    alternatives: [],
  },
  {
    name: "title_t07.mkv", durationS: 176, episode: { season: 4, number: 3 }, verdict: "confident",
    score: 0.92, margin: 0.47, chapter: 7,
    signals: { dialogue: 0.85, titleHook: 0.95, duration: 0.97, discOrder: 0.93 },
    notes: [{ kind: "titleHeard" }, { kind: "discOrderAgrees", chapter: 7 }],
    heard: quote(["…", false], ["a noun is a person place or thing", true], ["…", false]),
    reference: quote(["…", false], ["A noun is a person, place or thing", true], ["…", false]),
    alternatives: [],
  },
  {
    name: "title_t08.mkv", durationS: 190, episode: { season: 4, number: 4 }, verdict: "check",
    score: 0.58, margin: 0.06, chapter: 8,
    signals: { dialogue: 0.44, titleHook: 0.3, duration: 0.9, discOrder: 0.81 },
    notes: [{ kind: "mostlyMusic" }, { kind: "discOrderAgrees", chapter: 8 }, { kind: "sampled", windows: 1 }],
    heard: quote(["…he's got the ", false], ["verb", true], [", that's what's ", false], ["happening", true], ["…", false]),
    reference: quote(["…", false], ["Verb", true], ["! That's what's ", false], ["happening", true], ["…", false]),
    alternatives: [
      { episode: { season: 4, number: 6 }, score: 0.52, signals: { dialogue: 0.4, titleHook: 0, duration: 0.88, discOrder: 0.6 } },
    ],
  },
  {
    name: "title_t09.mkv", durationS: 184, episode: { season: 4, number: 5 }, verdict: "confident",
    score: 0.91, margin: 0.45, chapter: 9,
    signals: { dialogue: 0.84, titleHook: 0.9, duration: 0.96, discOrder: 0.94 },
    notes: [{ kind: "titleHeard" }, { kind: "discOrderAgrees", chapter: 9 }],
    heard: quote(["…", false], ["interjections", true], [" show excitement or emotion…", false]),
    reference: quote(["…", false], ["Interjections", true], [" show excitement or emotion…", false]),
    alternatives: [],
  },
  {
    name: "title_t11.mkv", durationS: 185, episode: { season: 2, number: 3 }, verdict: "check",
    score: 0.61, margin: 0.08, chapter: 10,
    signals: { dialogue: 0.48, titleHook: 0.7, duration: 0.8, discOrder: 0.95 },
    notes: [{ kind: "mostlyMusic" }, { kind: "discOrderAgrees", chapter: 10 }],
    heard: quote(["…", false], ["lucky seven", true], [", he's got the rhythm… ", false], ["seven fourteen twenty-one", true], ["…", false]),
    reference: quote(["…", false], ["Lucky Seven", true], [" Sampson… ", false], ["seven, fourteen, twenty-one", true], [", twenty-eight…", false]),
    alternatives: [
      { episode: { season: 2, number: 2 }, score: 0.53, signals: { dialogue: 0.4, titleHook: 0, duration: 0.8, discOrder: 0.6 } },
      { episode: { season: 2, number: 4 }, score: 0.41, signals: { dialogue: 0.31, titleHook: 0, duration: 0.82, discOrder: 0.4 } },
    ],
  },
  {
    name: "title_t12.mkv", durationS: 178, episode: { season: 2, number: 4 }, verdict: "confident",
    score: 0.93, margin: 0.5, chapter: 11,
    signals: { dialogue: 0.87, titleHook: 1, duration: 0.96, discOrder: 0.95 },
    notes: [{ kind: "titleHeard" }, { kind: "discOrderAgrees", chapter: 11 }],
    heard: quote(["…", false], ["figure eight", true], [" is double four…", false]),
    reference: quote(["…", false], ["Figure eight", true], [" is double four…", false]),
    alternatives: [],
  },
  {
    name: "title_t13.mkv", durationS: 187, episode: { season: 2, number: 7 }, verdict: "check",
    score: 0.55, margin: 0.09, chapter: 13,
    signals: { dialogue: null, titleHook: 0.6, duration: 0.95, discOrder: 0.9 },
    notes: [{ kind: "noReferenceText" }, { kind: "discOrderAgrees", chapter: 13 }],
    heard: quote(["…", false], ["naughty number nine", true], ["…", false]),
    reference: [],
    alternatives: [
      { episode: { season: 4, number: 6 }, score: 0.46, signals: { dialogue: null, titleHook: 0, duration: 0.93, discOrder: 0.5 } },
    ],
  },
  {
    name: "title_t44.mkv", durationS: 501, episode: null, verdict: "extra",
    score: 0.12, margin: 0.13, chapter: null,
    signals: { dialogue: 0.12, titleHook: 0, duration: 0.1, discOrder: null },
    notes: [],
    heard: [],
    reference: [],
    alternatives: [],
  },
  {
    name: "title_t45.mkv", durationS: 1390, episode: null, verdict: "extra",
    score: 0.08, margin: 0.2, chapter: null,
    signals: { dialogue: 0.08, titleHook: 0, duration: 0, discOrder: null },
    notes: [],
    heard: [],
    reference: [],
    alternatives: [],
  },
];

const PLAY_ALL_CHAPTERS = 14;
const PLAY_ALL_S = 2585;

export const SCAN: ScanSummary = {
  folder: SAMPLE_FOLDER,
  files: [
    file("title_t00.mkv", PLAY_ALL_S, "playAll", PLAY_ALL_CHAPTERS),
    ...SAMPLE_FILES.map((f) => file(f.name, f.durationS, "candidate")),
  ],
  playAll: {
    fileId: "title_t00.mkv",
    durationS: PLAY_ALL_S,
    chapterCount: PLAY_ALL_CHAPTERS,
    candidatesTotalS: SAMPLE_FILES.filter((f) => f.durationS < 360).reduce((t, f) => t + f.durationS, 0),
    chaptersMatched: SAMPLE_FILES.filter((f) => f.chapter !== null).length,
    confidence: 0.86,
    reason:
      "title_t00.mkv is about as long as the other files together, and most of them have the length of one of its chapters.",
  },
  candidateCount: SAMPLE_FILES.length,
  showGuess: "Schoolhouse Rock",
  warnings: [
    {
      kind: "missingShortTitles",
      chapters: PLAY_ALL_CHAPTERS,
      shortFiles: SAMPLE_FILES.filter((f) => f.durationS < 360).length,
    },
  ],
};

function chapterPosition(chapter: number | null, durationS: number, orderIndex: number) {
  if (chapter === null) return null;
  return { chapter, startS: chapter * 185, endS: chapter * 185 + durationS, orderIndex, alignmentScore: 0.93 };
}

function toMatch(f: SampleFile, orderIndex: number): FileMatch {
  const evidenceFor = (signals: Signals, full: boolean) => ({
    signals,
    heard: full ? f.heard : [],
    reference: full ? f.reference : [],
    playAllPosition: chapterPosition(f.chapter, f.durationS, orderIndex),
    notes: full ? f.notes : [],
  });
  const candidates: Candidate[] = f.episode
    ? [
        { episode: f.episode, title: title(f.episode), score: f.score, evidence: evidenceFor(f.signals, true) },
        ...f.alternatives.map((a) => ({
          episode: a.episode,
          title: title(a.episode),
          score: a.score,
          evidence: evidenceFor(a.signals, false),
        })),
      ]
    : [];
  return {
    fileId: f.name,
    suggestion: f.episode ? { kind: "episode", episode: f.episode } : { kind: "notAnEpisode" },
    confidence: { score: f.score, margin: f.margin, verdict: f.verdict },
    candidates,
  };
}

/** One result per file: the play-all first, then the files in disc order. */
export const MATCHES: FileMatch[] = [
  {
    fileId: "title_t00.mkv",
    suggestion: { kind: "playAll" },
    confidence: { score: 1, margin: 1, verdict: "playAll" },
    candidates: [],
  },
  ...SAMPLE_FILES.map((f, i) => toMatch(f, i)),
];

const DAY = 86_400_000;

/** Recent jobs relative to `now`, so "Yesterday" stays yesterday. */
export function recentJobs(now: number = Date.now()): RecentJob[] {
  return [
    {
      jobId: "job-disc2",
      folder: "/Volumes/Rips/SCHOOLHOUSE_ROCK_D2",
      showName: "Schoolhouse Rock! Disc 2",
      fileCount: 48,
      toReview: 0,
      saved: true,
      finishedAtMs: now - DAY,
    },
    {
      jobId: "job-mwc",
      folder: "/Volumes/Rips/MARRIED_WITH_CHILDREN_S03",
      showName: "Married... with Children",
      fileCount: 22,
      toReview: 2,
      saved: false,
      finishedAtMs: now - 2 * DAY,
    },
  ];
}

/** History entries relative to `now`. */
export function historyEntries(now: number = Date.now()): HistoryEntry[] {
  const folder = "/Volumes/Rips/SCHOOLHOUSE_ROCK_D2";
  const show = "Schoolhouse Rock! (1973)";
  const items = [
    ["title_t01.mkv", "S01E01 - My Hero, Zero"],
    ["title_t02.mkv", "S01E02 - Elementary, My Dear"],
    ["title_t03.mkv", "S01E03 - Three Is a Magic Number"],
  ].map(([from, name]) => ({
    from: `${folder}/${from}`,
    to: `${folder}/${show}/Season 01/${show} - ${name}.mkv`,
  }));
  return [
    {
      id: "hist-1",
      createdAtMs: now - DAY + 5 * 60_000,
      showName: "Schoolhouse Rock!",
      folder,
      mode: "renameInPlace",
      items,
      undoneAtMs: null,
    },
  ];
}

export const SOURCES: SourceStatus[] = [
  { provider: "tvmaze", state: { kind: "ready" }, hasKey: false },
  { provider: "lrclib", state: { kind: "ready" }, hasKey: false },
  { provider: "subdl", state: { kind: "needsKey" }, hasKey: false },
  { provider: "tmdb", state: { kind: "needsKey" }, hasKey: false },
];
