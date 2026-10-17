import { useEffect, useRef, useState } from 'react';
import type { Segment } from '../../lib/transcript/types';
import {
  PLAYBACK_RATES,
  clampSeek,
  formatClock,
  normalizeRate,
  type PlaybackRate,
  type SeekRequest,
} from './playback';

type AudioPlayerProps = {
  /** Asset-protocol URL for the recording's normalized WAV, or null while unavailable. */
  src: string | null;
  title: string;
  /** Duration from the library record, used until the media element reports its own. */
  durationMs: number;
  /** Playhead owned by the caller, so the transcript highlight and the player agree. */
  currentMs: number;
  /** Segment covering the playhead, or null when the playhead is in a gap. */
  activeSegment: Segment | null;
  /** A caller-initiated seek (for example from a transcript timestamp). */
  seekRequest: SeekRequest | null;
  loading: boolean;
  /** Why the recording's audio could not be prepared, if it could not. */
  error: string | null;
  onTimeUpdate: (ms: number) => void;
  onSeek: (ms: number) => void;
};

/**
 * Local playback for one recording. The component owns the media element and
 * reports playhead changes upward; the caller owns the playhead it displays.
 * It is keyed by the caller per recording, so switching recordings remounts it
 * and stops the previous audio.
 */
export default function AudioPlayer({
  src,
  title,
  durationMs,
  currentMs,
  activeSegment,
  seekRequest,
  loading,
  error,
  onTimeUpdate,
  onSeek,
}: AudioPlayerProps) {
  const audioRef = useRef<HTMLAudioElement | null>(null);
  // Latest callbacks and duration, read from callbacks that are not re-created
  // on every render (the animation-frame loop and the media-event handlers).
  const callbacks = useRef({ onTimeUpdate, onSeek });
  const totalMsRef = useRef(durationMs);
  const scrubbing = useRef(false);
  const pendingSeek = useRef<number | null>(null);

  useEffect(() => {
    const audio = audioRef.current;
    return () => { if (audio) { audio.pause(); audio.removeAttribute('src'); audio.load(); } };
  }, []);

  const [playing, setPlaying] = useState(false);
  const [rate, setRate] = useState<PlaybackRate>(1);
  const [mediaDurationMs, setMediaDurationMs] = useState(0);
  const [mediaError, setMediaError] = useState<string | null>(null);

  const totalMs = mediaDurationMs > 0 ? mediaDurationMs : durationMs;
  const positionMs = clampSeek(currentMs, totalMs > 0 ? totalMs : undefined);
  const remainingMs = Math.max(totalMs - positionMs, 0);
  const disabled = !src || Boolean(error) || loading;

  useEffect(() => {
    callbacks.current = { onTimeUpdate, onSeek };
  });
  useEffect(() => {
    totalMsRef.current = totalMs;
  });
  // Apply the chosen speed whenever it changes, and again after each load
  // (see `applyMediaDuration`), because a new source re-initialises playback.
  useEffect(() => {
    const audio = audioRef.current;
    if (audio) audio.playbackRate = rate;
  }, [rate]);

  // While playing, report a smoothed playhead so the highlight tracks the
  // audio between the media element's less frequent `timeupdate` events.
  useEffect(() => {
    if (!playing) return;
    let frame = 0;
    let last = 0;
    const tick = (now: number) => {
      frame = requestAnimationFrame(tick);
      if (now - last < 100) return;
      last = now;
      const audio = audioRef.current;
      if (audio && !scrubbing.current) {
        callbacks.current.onTimeUpdate(audio.currentTime * 1000);
      }
    };
    frame = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(frame);
  }, [playing]);

  // Move the playhead when the caller asks for a seek (a timestamp click).
  useEffect(() => {
    const audio = audioRef.current;
    if (!seekRequest || !audio) return;
    const target = clampSeek(
      seekRequest.ms,
      totalMsRef.current > 0 ? totalMsRef.current : undefined,
    );
    if (audio.readyState > 0) audio.currentTime = target / 1000;
    else pendingSeek.current = target;
    callbacks.current.onSeek(target);
  }, [seekRequest]);

  function applyMediaDuration(audio: HTMLAudioElement) {
    if (Number.isFinite(audio.duration) && audio.duration > 0) {
      setMediaDurationMs(audio.duration * 1000);
    }
    audio.playbackRate = rate;
    if (pendingSeek.current !== null) { audio.currentTime = clampSeek(pendingSeek.current, audio.duration * 1000) / 1000; pendingSeek.current = null; }
  }

  function toggle() {
    const audio = audioRef.current;
    if (!audio || !src) return;
    if (audio.paused) {
      void audio.play().catch(() => {
        setPlaying(false);
        setMediaError('Playback could not start. Check that the audio is available and try again.');
      });
    } else {
      audio.pause();
    }
  }

  function seek(ms: number) {
    const target = clampSeek(ms, totalMs > 0 ? totalMs : undefined);
    const audio = audioRef.current;
    if (audio && audio.readyState > 0) audio.currentTime = target / 1000;
    else pendingSeek.current = target;
    callbacks.current.onSeek(target);
  }

  const message = error ?? mediaError;

  return (
    <section className="audio-player" aria-label={`Audio player for ${title}`}>
      <audio
        ref={audioRef}
        src={src ?? undefined}
        preload="metadata"
        onPlay={() => {
          setMediaError(null);
          setPlaying(true);
        }}
        onPause={() => setPlaying(false)}
        onEnded={() => setPlaying(false)}
        onLoadedMetadata={event => applyMediaDuration(event.currentTarget)}
        onDurationChange={event => applyMediaDuration(event.currentTarget)}
        onTimeUpdate={event => {
          if (!scrubbing.current) {
            callbacks.current.onTimeUpdate(event.currentTarget.currentTime * 1000);
          }
        }}
        onError={() => {
          setPlaying(false);
          setMediaError('This recording’s audio could not be played.');
        }}
      />
      <div className="audio-player-controls">
        <button
          type="button"
          className="play-button"
          onClick={toggle}
          disabled={disabled}
          aria-label={playing ? 'Pause' : 'Play'}
        >
          <span aria-hidden="true">{playing ? '❚❚' : '▶'}</span>
        </button>
        <div className="audio-player-timeline">
          <input
            type="range"
            className="scrubber"
            min={0}
            max={Math.max(totalMs, 1)}
            step={100}
            value={positionMs}
            disabled={disabled}
            aria-label="Seek within the recording"
            aria-valuetext={`${formatClock(positionMs)} of ${formatClock(Math.max(totalMs, 0))}`}
            onChange={event => seek(Number(event.target.value))}
            onPointerDown={() => {
              scrubbing.current = true;
            }}
            onPointerUp={() => {
              scrubbing.current = false;
            }}
            onPointerCancel={() => {
              scrubbing.current = false;
            }}
            onBlur={() => {
              scrubbing.current = false;
            }}
          />
          <div className="audio-player-times">
            <span>{formatClock(positionMs)}</span>
            <span>{`−${formatClock(remainingMs)}`}</span>
          </div>
        </div>
        <label className="audio-player-rate">
          Speed
          <select
            value={rate}
            disabled={disabled}
            onChange={event => setRate(normalizeRate(Number(event.target.value)))}
          >
            {PLAYBACK_RATES.map(value => (
              <option key={value} value={value}>{`${value}×`}</option>
            ))}
          </select>
        </label>
      </div>
      {activeSegment && (
        <p className="audio-player-now">
          <span className="audio-player-now-label">Now</span> {activeSegment.text}
        </p>
      )}
      {loading && <p className="audio-player-status" role="status">Preparing audio…</p>}
      {message && <p className="audio-player-status error-text" role="alert">{message}</p>}
      {!src && !loading && !message && (
        <p className="audio-player-status">Audio is unavailable for this recording.</p>
      )}
    </section>
  );
}
