// Sample data for the mock backend, modelled on the approved mockup (Schoolhouse Rock disc 1).
// Fictional results for UI development and tests; not real identification output.

import type {
  Episode,
  FileMatch,
  HistoryEntry,
  MediaFile,
  RecentJob,
  ScanSummary,
  Show,
  ShowCandidate,
  SourceStatus,
} from "../types/generated";

export const SAMPLE_FOLDER = "/Volumes/Rips/SCHOOLHOUSE_ROCK_D1";

export const SCHOOLHOUSE_ROCK: Show = {
  showRef: { provider: "tvmaze", id: "2617" },
  name: "Schoolhouse Rock!",
  year: 1973,
  kind: "Animation",
  seasonCount: 7,
  episodeCount: 64,
  url: "https://www.tvmaze.com/shows/2617/schoolhouse-rock",
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
  episode(2, 2, "Elementary, My Dear"),
  episode(2, 3, "Lucky Seven Sampson"),
  episode(2, 4, "Figure Eight"),
  episode(2, 7, "Naughty Number Nine"),
  episode(4, 1, "Conjunction Junction"),
  episode(4, 2, "Unpack Your Adjectives"),
];

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
        startS: i * 180,
        endS: (i + 1) * 180,
        title: null,
      })),
    },
    role,
  };
}

export const SCAN: ScanSummary = {
  folder: SAMPLE_FOLDER,
  files: [
    file("title_t00.mkv", 8091, "playAll", 52),
    file("title_t03.mkv", 192, "candidate"),
    file("title_t04.mkv", 185, "candidate"),
    file("title_t11.mkv", 185, "candidate"),
    file("title_t12.mkv", 178, "candidate"),
    file("title_t44.mkv", 501, "candidate"),
  ],
  playAll: { fileId: "title_t00.mkv", durationS: 8091, chapterCount: 52, candidatesTotalS: 7650 },
  candidateCount: 5,
  showGuess: "Schoolhouse Rock",
  warnings: [{ kind: "missingShortTitles", chapters: 52, shortFiles: 46 }],
};

export const MATCHES: FileMatch[] = [
  {
    fileId: "title_t03.mkv",
    suggestion: { kind: "episode", episode: { season: 4, number: 1 } },
    confidence: { score: 0.97, margin: 0.62, verdict: "confident" },
    candidates: [
      {
        episode: { season: 4, number: 1 },
        title: "Conjunction Junction",
        score: 0.97,
        evidence: {
          signals: { dialogue: 0.91, titleHook: 1, duration: 0.95, discOrder: 0.98 },
          heard: [
            { text: "conjunction junction", matched: true },
            { text: ", what's your ", matched: false },
            { text: "function", matched: true },
          ],
          reference: [
            { text: "Conjunction Junction", matched: true },
            { text: ", what's your ", matched: false },
            { text: "function?", matched: true },
          ],
          playAllPosition: { chapter: 3, startS: 540, endS: 732, orderIndex: 3, alignmentScore: 0.96 },
          notes: [{ kind: "titleHeard" }, { kind: "discOrderAgrees", chapter: 3 }],
        },
      },
    ],
  },
  {
    fileId: "title_t11.mkv",
    suggestion: { kind: "episode", episode: { season: 2, number: 3 } },
    confidence: { score: 0.61, margin: 0.08, verdict: "check" },
    candidates: [
      {
        episode: { season: 2, number: 3 },
        title: "Lucky Seven Sampson",
        score: 0.61,
        evidence: {
          signals: { dialogue: 0.48, titleHook: 0.7, duration: 0.8, discOrder: 0.95 },
          heard: [
            { text: "… ", matched: false },
            { text: "lucky seven", matched: true },
            { text: ", he's got the rhythm… ", matched: false },
            { text: "seven fourteen twenty-one", matched: true },
            { text: "…", matched: false },
          ],
          reference: [
            { text: "… ", matched: false },
            { text: "Lucky Seven", matched: true },
            { text: " Sampson… ", matched: false },
            { text: "seven, fourteen, twenty-one", matched: true },
            { text: ", twenty-eight…", matched: false },
          ],
          playAllPosition: { chapter: 11, startS: 1980, endS: 2165, orderIndex: 11, alignmentScore: 0.9 },
          notes: [{ kind: "mostlyMusic" }, { kind: "discOrderAgrees", chapter: 11 }],
        },
      },
      {
        episode: { season: 2, number: 2 },
        title: "Elementary, My Dear",
        score: 0.53,
        evidence: {
          signals: { dialogue: 0.4, titleHook: 0, duration: 0.8, discOrder: 0.6 },
          heard: [],
          reference: [],
          playAllPosition: null,
          notes: [],
        },
      },
    ],
  },
  {
    fileId: "title_t44.mkv",
    suggestion: { kind: "notAnEpisode" },
    confidence: { score: 0.12, margin: 0.13, verdict: "extra" },
    candidates: [],
  },
  {
    fileId: "title_t00.mkv",
    suggestion: { kind: "playAll" },
    confidence: { score: 1, margin: 1, verdict: "playAll" },
    candidates: [],
  },
];

export const RECENT: RecentJob[] = [
  {
    jobId: "job-disc2",
    folder: "/Volumes/Rips/SCHOOLHOUSE_ROCK_D2",
    showName: "Schoolhouse Rock!",
    fileCount: 48,
    toReview: 0,
    saved: true,
    finishedAtMs: Date.UTC(2026, 9, 4, 18, 0),
  },
  {
    jobId: "job-mwc",
    folder: "/Volumes/Rips/MARRIED_WITH_CHILDREN_S03",
    showName: "Married... with Children",
    fileCount: 22,
    toReview: 2,
    saved: false,
    finishedAtMs: Date.UTC(2026, 9, 3, 18, 0),
  },
];

export const HISTORY: HistoryEntry[] = [
  {
    id: "hist-1",
    createdAtMs: Date.UTC(2026, 9, 4, 18, 5),
    showName: "Schoolhouse Rock!",
    folder: "/Volumes/Rips/SCHOOLHOUSE_ROCK_D2",
    mode: "renameInPlace",
    items: [
      {
        from: "/Volumes/Rips/SCHOOLHOUSE_ROCK_D2/title_t01.mkv",
        to: "/Volumes/Rips/SCHOOLHOUSE_ROCK_D2/Schoolhouse Rock! (1973)/Season 01/Schoolhouse Rock! (1973) - S01E01 - My Hero, Zero.mkv",
      },
    ],
    undoneAtMs: null,
  },
];

export const SOURCES: SourceStatus[] = [
  { provider: "tvmaze", state: { kind: "ready" }, hasKey: false },
  { provider: "lrclib", state: { kind: "ready" }, hasKey: false },
  { provider: "subdl", state: { kind: "needsKey" }, hasKey: false },
  { provider: "tmdb", state: { kind: "needsKey" }, hasKey: false },
];
