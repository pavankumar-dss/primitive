package primitive

import (
	"image"
	"image/color"
	"math"
	"testing"
)

func constrainedModel(t *testing.T, c Constraints, shapeType ShapeType, shapes int) *Model {
	t.Helper()
	model := testModel(t, 0)
	model.SetConstraints(c)
	for i := 0; i < shapes; i++ {
		model.Step(shapeType, 128, 0)
	}
	if len(model.Shapes) == 0 {
		t.Fatal("no shapes were added")
	}
	return model
}

// angleDiff returns the difference between two axis directions in degrees,
// ignoring which end points where.
func angleDiff(a, b float64) float64 {
	return math.Abs(math.Mod(a-b+3690, 180) - 90)
}

func TestFixedAngle(t *testing.T) {
	model := constrainedModel(t, Constraints{FixedAngle: true, Angle: 30}, ShapeTypeRotatedEllipse, 10)
	for _, s := range model.Shapes {
		e := s.(*RotatedEllipse)
		long := e.Angle
		if e.Rx < e.Ry {
			long += 90
		}
		if d := angleDiff(long, -30); d > 1e-6 {
			t.Fatalf("ellipse long axis at %.1f degrees, want -30 (diff %.2f)", long, d)
		}
		if math.Max(e.Rx, e.Ry) < 2*math.Min(e.Rx, e.Ry)-1e-9 {
			t.Fatalf("ellipse not elongated: %.1f x %.1f", e.Rx, e.Ry)
		}
	}
}

func TestFocus(t *testing.T) {
	c := Constraints{Focus: true, FocusX: 0.25, FocusY: 0.75}
	model := constrainedModel(t, c, ShapeTypeRotatedRectangle, 10)
	fx, fy := 0.25*64, 0.75*48
	for _, s := range model.Shapes {
		r := s.(*RotatedRectangle)
		long := float64(r.Angle)
		if r.Sx < r.Sy {
			long += 90
		}
		want := degrees(math.Atan2(fy-float64(r.Y), fx-float64(r.X)))
		if d := angleDiff(long, want); d > 0.5 {
			t.Fatalf("rectangle at (%d,%d) points %.1f degrees, want %.1f", r.X, r.Y, long, want)
		}
	}
}

func TestSizeLimits(t *testing.T) {
	model := constrainedModel(t, Constraints{MaxSize: 0.2}, ShapeTypeTriangle, 10)
	for _, s := range model.Shapes {
		if !model.Workers[0].Allowed(s) {
			t.Fatal("a shape breaks the size limit")
		}
		lines := s.Rasterize()
		x0, x1, y0, y1 := 1<<30, -1, 1<<30, -1
		for _, l := range lines {
			x0, x1 = min(x0, l.X1), max(x1, l.X2)
			y0, y1 = min(y0, l.Y), max(y1, l.Y)
		}
		if size := max(x1-x0, y1-y0) + 1; float64(size) > 0.2*64 {
			t.Fatalf("shape is %d px, limit is %.1f", size, 0.2*64)
		}
	}
}

func TestRegion(t *testing.T) {
	// only the left half of the image is allowed
	region := image.NewGray(image.Rect(0, 0, 64, 48))
	for y := 0; y < 48; y++ {
		for x := 0; x < 32; x++ {
			region.SetGray(x, y, color.Gray{255})
		}
	}
	model := constrainedModel(t, Constraints{Region: region}, ShapeTypeTriangle, 10)
	for _, s := range model.Shapes {
		var inside, total int
		for _, l := range s.Rasterize() {
			total += l.X2 - l.X1 + 1
			inside += max(0, min(l.X2, 31)-l.X1+1)
		}
		if float64(inside) < (1-regionTolerance)*float64(total)-1e-9 {
			t.Fatalf("only %d of %d pixels inside the region", inside, total)
		}
	}
}
