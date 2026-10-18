import { useEffect, useMemo, useRef, useState } from 'react';
import type { Segment } from '../../lib/transcript/types';
import { api } from './api';
import { findTranscriptMatches, formatTime, segmentAtTime } from './review';

type Props = {
  noteId: string;
  durationSeconds: number;
  segments: Segment[];
  disabled: boolean;
  onChange: (index: number, text: string) => void;
};

export default function RecordingReview({ noteId, durationSeconds, segments, disabled, onChange }: Props) {
  const audio = useRef<HTMLAudioElement>(null);
  const editors = useRef(new Map<number, HTMLTextAreaElement>());
  const [source, setSource] = useState<string | undefined>();
  const [audioError, setAudioError] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);
  const [ready, setReady] = useState(false);
  const [playing, setPlaying] = useState(false);
  const [position, setPosition] = useState(0);
  const [duration, setDuration] = useState(durationSeconds);
  const [rate, setRate] = useState(1);
  const [query, setQuery] = useState('');
  const [matchIndex, setMatchIndex] = useState(-1);
  const matches = useMemo(() => findTranscriptMatches(segments, query), [segments, query]);
  const matchingSegments = useMemo(() => new Set(matches.map(match => match.segmentIndex)), [matches]);
  const selectedMatch = matchIndex < 0 ? -1 : Math.min(matchIndex, matches.length - 1);
  const currentSegment = ready ? segmentAtTime(segments, position) : -1;
  const canPlay = ready && !audioError && !disabled;

  useEffect(() => {
    let disposed = false;
    const element = audio.current;
    api.audioSource(noteId).then(url => {
      if (!disposed) setSource(url);
    }).catch(error => {
      if (!disposed) setAudioError(typeof error === 'string' ? error : 'Could not open local audio. Try again.');
    });
    return () => {
      disposed = true;
      if (element) { element.pause(); element.removeAttribute('src'); element.load(); }
    };
  }, [noteId, attempt]);

  async function play() {
    const element = audio.current;
    if (!element || !canPlay) return;
    try { await element.play(); }
    catch (error) {
      // Changing recordings or pausing during a pending play request is normal.
      if (!(error instanceof DOMException && error.name === 'AbortError')) {
        setAudioError('Could not play this recording. Try loading the audio again.');
      }
    }
  }
  function togglePlayback() {
    if (audio.current?.paused) void play();
    else audio.current?.pause();
  }
  function seek(seconds: number, startPlaying = false) {
    const element = audio.current;
    if (!element || !canPlay) return;
    const next = Math.max(0, Math.min(duration, seconds));
    element.currentTime = next;
    setPosition(next);
    if (startPlaying) void play();
  }
  function navigateMatch(direction: 1 | -1) {
    if (!matches.length) return;
    const index = selectedMatch < 0 ? (direction === 1 ? 0 : matches.length - 1)
      : (selectedMatch + direction + matches.length) % matches.length;
    const match = matches[index];
    setMatchIndex(index);
    const editor = editors.current.get(match.segmentIndex);
    if (editor) {
      editor.scrollIntoView({ block: 'center' });
      editor.focus({ preventScroll: true });
      editor.setSelectionRange(match.start, match.end);
    }
  }
  function retryAudio() {
    setSource(undefined); setReady(false); setPlaying(false); setPosition(0); setAudioError(null);
    setAttempt(value => value + 1);
  }

  return <>
    <section className="audio-player" aria-label="Audio playback" tabIndex={0}
      aria-keyshortcuts="Space ArrowLeft ArrowRight" onKeyDown={event => {
        if (event.target !== event.currentTarget || !canPlay) return;
        if (event.key === ' ') { event.preventDefault(); togglePlayback(); }
        if (event.key === 'ArrowLeft') { event.preventDefault(); seek(position - 10); }
        if (event.key === 'ArrowRight') { event.preventDefault(); seek(position + 10); }
      }}>
      {/* The visible transcript provides the audio's text equivalent. */}
      <audio ref={audio} src={source} preload="metadata" onLoadedMetadata={event => {
        const element = event.currentTarget;
        if (!Number.isFinite(element.duration) || element.duration <= 0) {
          setAudioError('This recording has no playable audio.'); return;
        }
        setDuration(element.duration); setReady(true); element.playbackRate = rate;
      }} onTimeUpdate={event => setPosition(event.currentTarget.currentTime)}
      onPlay={() => setPlaying(true)} onPause={() => setPlaying(false)} onEnded={() => setPlaying(false)}
      onError={() => { if (audio.current?.getAttribute('src')) setAudioError('Audio could not be loaded. Your transcript is still available.'); }}/>
      <div className="player-heading"><span className="eyebrow">LISTEN & REVIEW</span>
        <span className="player-hint">Focus player · Space to play · ← → skip 10s</span></div>
      {audioError ? <div className="audio-error" role="alert"><p>{audioError}</p>
        <button className="secondary" disabled={disabled} onClick={retryAudio}>Retry audio</button></div>
        : <>
          <div className="player-controls">
            <button className="primary play-button" disabled={!canPlay} onClick={togglePlayback} aria-label={playing ? 'Pause recording' : 'Play recording'}>
              <span aria-hidden="true">{playing ? 'Ⅱ' : '▶'}</span> {playing ? 'Pause' : 'Play'}
            </button>
            <button className="secondary skip-button" disabled={!canPlay} onClick={() => seek(position - 10)} aria-label="Back 10 seconds">−10s</button>
            <button className="secondary skip-button" disabled={!canPlay} onClick={() => seek(position + 10)} aria-label="Forward 10 seconds">+10s</button>
            <span className="playback-time">{formatTime(position)} / {formatTime(duration)}</span>
            <label className="playback-speed">Speed<select value={rate} disabled={!canPlay} onChange={event => {
              const next = Number(event.target.value); setRate(next);
              if (audio.current) audio.current.playbackRate = next;
            }}>{[0.5, 0.75, 1, 1.25, 1.5, 2].map(value => <option key={value} value={value}>{value}×</option>)}</select></label>
          </div>
          <input className="playback-seek" type="range" aria-label="Playback position" aria-valuetext={`${formatTime(position)} of ${formatTime(duration)}`}
            min={0} max={duration} step={0.1} value={position} disabled={!canPlay} onChange={event => seek(Number(event.target.value))}/>
          {!ready && <p className="muted" role="status">Loading local audio…</p>}
        </>}
    </section>

    {!!segments.length && <div className="transcript-search" role="search" aria-label="Search transcript">
      <label className="sr-only" htmlFor="transcript-search">Find in transcript</label>
      <input id="transcript-search" type="search" placeholder="Find in transcript…" value={query} onChange={event => {
        setQuery(event.target.value); setMatchIndex(-1);
      }} onKeyDown={event => {
        if (event.key === 'Enter') { event.preventDefault(); navigateMatch(event.shiftKey ? -1 : 1); }
        if (event.key === 'Escape') { setQuery(''); setMatchIndex(-1); }
      }}/>
      <span className="search-count" role="status">{query.trim() ? matches.length
        ? selectedMatch < 0 ? `${matches.length} ${matches.length === 1 ? 'match' : 'matches'}` : `${selectedMatch + 1} of ${matches.length}`
        : 'No matches' : 'Search your edits too'}</span>
      <button className="secondary" disabled={!matches.length} aria-label="Previous match" onClick={() => navigateMatch(-1)}>↑</button>
      <button className="secondary" disabled={!matches.length} aria-label="Next match" onClick={() => navigateMatch(1)}>↓</button>
    </div>}

    <div className="transcript">{segments.map((segment, index) => <div key={index}
      className={`segment${currentSegment === index ? ' current-segment' : ''}${matchingSegments.has(index) ? ' search-match' : ''}${matches[selectedMatch]?.segmentIndex === index ? ' selected-match' : ''}`}>
      <button className="timecode" disabled={!canPlay} aria-label={`Play from ${formatTime(segment.startMs / 1000)}`}
        aria-current={currentSegment === index ? 'true' : undefined} onClick={() => seek(segment.startMs / 1000, true)}>{formatTime(segment.startMs / 1000)}</button>
      <textarea ref={element => { if (element) editors.current.set(index, element); else editors.current.delete(index); }}
        aria-label={`Transcript at ${formatTime(segment.startMs / 1000)}`} value={segment.text} disabled={disabled}
        rows={Math.max(2, Math.ceil(segment.text.length / 85))} onChange={event => onChange(index, event.target.value)}/>
    </div>)}</div>
  </>;
}
