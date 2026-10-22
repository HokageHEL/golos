import Disclosure from './Disclosure';
import type { ExportOptionsState, SpeakerGrouping } from './export/options';

type Props = {
  value: ExportOptionsState;
  disabled: boolean;
  onChange: (patch: Partial<ExportOptionsState>) => void;
};

export default function ExportSettings({ value, disabled, onChange }: Props) {
  const customDisabled = disabled || !value.custom;
  return <Disclosure className="export-options" summary="Export options">
    <div className="export-options-body">
      <label className="export-custom"><input type="checkbox" checked={value.custom} disabled={disabled} onChange={event => onChange({ custom: event.target.checked })}/> Customize export layout</label>
      <p className="export-options-note">Off by default: TXT, Markdown, HTML, DOCX, SRT and VTT keep their existing output unless you turn this on. Options are not saved.</p>
      <div className="export-options-grid">
        <fieldset disabled={customDisabled}>
          <legend>Readable formats: TXT, Markdown, HTML, DOCX</legend>
          <label><input type="checkbox" checked={value.includeTimestamps} onChange={event => onChange({ includeTimestamps: event.target.checked })}/> Include timestamps</label>
          <label><input type="checkbox" checked={value.includeSpeakers} onChange={event => onChange({ includeSpeakers: event.target.checked })}/> Include speaker labels</label>
          <label>Grouping<select aria-label="Paragraph grouping" value={value.grouping} onChange={event => onChange({ grouping: event.target.value as SpeakerGrouping })}>
            <option value="paragraph">Paragraph — one saved segment per block</option>
            <option value="same-speaker">Adjacent same speaker — combine consecutive segments</option>
          </select></label>
        </fieldset>
        <fieldset disabled={customDisabled}>
          <legend>Subtitles: SRT, VTT</legend>
          <label>Timestamp offset (milliseconds)<input type="text" inputMode="text" maxLength={32} aria-label="Subtitle timestamp offset" value={value.offsetText} onChange={event => onChange({ offsetText: event.target.value })}/></label>
          <label>Line width<select aria-label="Subtitle line width" value={value.maxLineLength} onChange={event => onChange({ maxLineLength: Number(event.target.value) })}>
            <option value={0}>No wrapping</option>
            <option value={32}>32 characters</option>
            <option value={42}>42 characters</option>
            <option value={50}>50 characters</option>
          </select></label>
          <p className="export-options-note">Wrapping only adds line breaks inside one cue. Cues are never split and word-level times are never invented.</p>
        </fieldset>
      </div>
      <p className="export-options-note"><strong>Applies to manual export only.</strong> CSV and JSON always retain canonical timestamps, speakers, text and segment order. Saved audio and .golos project backups are unchanged. Automatic exports and AI results keep their defaults.</p>
    </div>
  </Disclosure>;
}
