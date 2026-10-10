import type { DateFilterState } from './mediaDates'
import './date-filter.css'

export default function DateFilter({ value, error, onChange }: { value: DateFilterState; error: string | null; onChange: (filter: DateFilterState) => void }) {
  return <section className="date-filter" aria-label="Filter by date">
    <div className="date-filter-controls">
      <label>Date <select aria-label="Date field" value={value.field} onChange={event => onChange({ ...value, field: event.target.value as DateFilterState['field'] })}><option value="modified">Modified</option><option value="created">Created</option></select></label>
      <select aria-label="Date range" value={value.preset} onChange={event => onChange({ ...value, preset: event.target.value as DateFilterState['preset'] })}><option value="all">All dates</option><option value="today">Today</option><option value="yesterday">Yesterday</option><option value="7">Last 7 days</option><option value="30">Last 30 days</option><option value="custom">Custom dates</option></select>
      {value.preset === 'custom' && <><label>From <input type="date" value={value.from} onChange={event => onChange({ ...value, from: event.target.value })} /></label><label>To <input type="date" value={value.to} onChange={event => onChange({ ...value, to: event.target.value })} /></label></>}
      <button disabled={value.preset === 'all'} onClick={() => onChange({ ...value, preset: 'all', from: '', to: '' })}>Clear date filter</button>
    </div>
    <p>{value.field === 'created' ? 'Filesystem creation date; unavailable dates are excluded when filtering. Copying can change it. ' : ''}Local timezone · From and To include the whole day. Last 7/30 days include today.</p>
    {error && <p className="date-filter-error" role="alert">{error}</p>}
  </section>
}
