import type { useTagFilter } from './useTagFilter'
import './tag-filter.css'

export default function TagFilter({ filter, count, total, onChange, workspace, onScopeChange, ready }: { filter: ReturnType<typeof useTagFilter>; count: number; total: number; onChange: (ids: number[], matchAll: boolean) => void; workspace: boolean; onScopeChange: (workspace: boolean) => void; ready: boolean }) {
  return <section className="tag-filter" aria-label="Filter videos by tag">
    <div className="tag-filter-controls"><label>Scope <select value={workspace ? 'workspace' : 'folder'} onChange={event => onScopeChange(event.target.value === 'workspace')}><option value="folder">Current folder</option><option value="workspace">Entire workspace</option></select></label><label>Tags <select aria-label="Add tag filter" value="" onChange={event => { if (event.target.value) onChange([...filter.ids, Number(event.target.value)], filter.matchAll) }}><option value="">Add a tag filter…</option>{filter.tags.filter(tag => !filter.ids.includes(tag.id)).map(tag => <option key={tag.id} value={tag.id}>{tag.name}</option>)}</select></label>
      <select aria-label="Tag matching mode" value={filter.matchAll ? 'all' : 'any'} onChange={event => onChange(filter.ids, event.target.value === 'all')}><option value="all">Match all</option><option value="any">Match any</option></select>
      <button onClick={() => onChange([], filter.matchAll)} disabled={!filter.active}>Clear filters</button>
      <span role="status">{!ready ? 'Loading results…' : `${count} of ${total} videos · ${workspace ? 'Entire workspace' : 'Current folder'}`}</span></div>
    {filter.active && <div className="active-tag-filters">{filter.ids.map(id => <button key={id} aria-label={`Remove filter ${filter.tags.find(tag => tag.id === id)?.name ?? id}`} onClick={() => onChange(filter.ids.filter(value => value !== id), filter.matchAll)}>{filter.tags.find(tag => tag.id === id)?.name ?? 'Tag'} <span aria-hidden="true">×</span></button>)}</div>}
    {filter.error && <p className="tag-filter-error" role="alert">Could not update tag filters: {filter.error}</p>}
  </section>
}

