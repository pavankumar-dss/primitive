package primitive

import (
	"bytes"
	"reflect"
	"testing"
)

func TestEncodeShapeRoundTrip(t *testing.T) {
	model := testModel(t, 0)
	worker := model.Workers[0]
	for st := ShapeTypeTriangle; st <= ShapeTypePolygon; st++ {
		for i := 0; i < 20; i++ {
			s := worker.RandomState(st, 128).Shape
			got, err := DecodeShape(worker, EncodeShape(s))
			if err != nil {
				t.Fatalf("shape type %d: %v", st, err)
			}
			if !reflect.DeepEqual(got, s) {
				t.Fatalf("shape type %d: decoded %+v, want %+v", st, got, s)
			}
		}
	}
	for _, v := range [][]float64{nil, {99}, {float64(ShapeTypeTriangle), 1, 2}, {float64(ShapeTypePolygon), 0, 1, 2}} {
		if _, err := DecodeShape(worker, v); err == nil {
			t.Errorf("DecodeShape(%v) succeeded, want an error", v)
		}
	}
}

// Shapes found by one model and committed to another, as the browser's Web
// Workers do, must leave both with the same image and score.
func TestCommitAndReplay(t *testing.T) {
	searcher, follower := testModel(t, 0), testModel(t, 0)
	for i := 0; i < 10; i++ {
		var best *State
		for k := 0; k < 3; k++ {
			if s := searcher.Search(ShapeTypeAny, 0, 1); best == nil || s.Energy() < best.Energy() {
				best = s
			}
		}
		for _, s := range searcher.Commit(best, 2) {
			shape, err := DecodeShape(follower.Workers[0], EncodeShape(s.Shape))
			if err != nil {
				t.Fatal(err)
			}
			follower.Add(shape, s.Alpha)
		}
	}
	if len(searcher.Shapes) < 10 {
		t.Fatalf("only %d shapes were committed", len(searcher.Shapes))
	}
	if follower.Score != searcher.Score || !bytes.Equal(follower.Current.Pix, searcher.Current.Pix) {
		t.Fatalf("follower diverged: score %v vs %v", follower.Score, searcher.Score)
	}
	if follower.SVG() != searcher.SVG() {
		t.Fatal("follower SVG differs")
	}
}
