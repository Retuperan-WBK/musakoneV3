import gleam/list
import gleeunit/should
import recommend

pub fn weighted_sample_picks_distinct_and_favours_weight_test() {
  let items = [#("heavy", 100.0), #("a", 1.0), #("b", 1.0), #("c", 1.0), #("d", 1.0)]

  let picked = recommend.weighted_sample(items, 3, fn(i) { i.1 })
  list.length(picked) |> should.equal(3)
  list.length(list.unique(picked)) |> should.equal(3)

  // Heavy item must land in nearly every 3-of-5 draw; the light ones must rotate
  let runs = list.range(1, 200)
  let heavy_hits =
    list.count(runs, fn(_) {
      recommend.weighted_sample(items, 3, fn(i) { i.1 })
      |> list.any(fn(i) { i.0 == "heavy" })
    })
  { heavy_hits > 190 } |> should.be_true
  let variety =
    runs
    |> list.flat_map(fn(_) { recommend.weighted_sample(items, 3, fn(i) { i.1 }) })
    |> list.unique
    |> list.length
  variety |> should.equal(5)
}
