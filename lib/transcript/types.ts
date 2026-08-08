export type Segment = {
  startMs: number;
  endMs: number;
  speaker: string | null;
  text: string;
};

export type Turn = {
  startMs: number;
  endMs: number;
  speaker: string | null;
  text: string;
};
