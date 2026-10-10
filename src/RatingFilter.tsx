import type { RatingFilterState } from './mediaRatings'
import './ratings.css'

export default function RatingFilter({ value, onChange }: { value: RatingFilterState; onChange: (value: RatingFilterState) => void }) {
  return <div className="rating-filter" aria-label="Rating filter">
    <label>Rating <select value={value.mode} onChange={event => onChange({ ...value, mode: event.target.value as RatingFilterState['mode'] })}>
      <option value="all">All ratings</option><option value="unrated">Unrated only</option><option value="exactly">Exactly</option><option value="atLeast">At least</option><option value="moreThan">More than</option>
    </select></label>
    {value.mode !== 'all' && value.mode !== 'unrated' && <label><span className="rating-sr-only">Number of stars</span><select value={value.value} onChange={event => onChange({ ...value, value: Number(event.target.value) })}>{[1, 2, 3, 4, 5].map(stars => <option key={stars} value={stars}>{stars} {stars === 1 ? 'star' : 'stars'}</option>)}</select></label>}
    {value.mode !== 'all' && <button onClick={() => onChange({ ...value, mode: 'all' })}>Clear rating filter</button>}
  </div>
}
