package primitive

import (
	"image"
	"math"

	xdraw "golang.org/x/image/draw"
)

// Constraints restrict the shapes the algorithm may choose. The zero value
// allows any shape.
type Constraints struct {
	// Focus makes oriented shapes (rotated rectangles, rotated ellipses and
	// Bézier curves) point their long axis toward (FocusX, FocusY), given
	// as fractions of the image width and height.
	Focus          bool
	FocusX, FocusY float64

	// FixedAngle makes oriented shapes point their long axis along Angle,
	// in degrees counterclockwise from horizontal. Focus takes precedence.
	FixedAngle bool
	Angle      float64

	// MinSize and MaxSize limit the longest side of each shape's bounding
	// box, as fractions of the image's longest side. Zero means no limit.
	MinSize, MaxSize float64

	// Region, if set, keeps shapes inside its bright, opaque pixels. It is
	// scaled to the image size.
	Region image.Image
}

// regionTolerance is the fraction of a shape allowed outside the region.
const regionTolerance = 0.1

// minOrientedAspect is the smallest length-to-width ratio of rectangles and
// ellipses under a direction constraint; rounder shapes show no direction.
const minOrientedAspect = 2.0

// constraints is Constraints resolved to working-image pixels.
type constraints struct {
	orient           bool
	focus            bool
	fx, fy           float64
	angle            float64 // degrees, clockwise (image y points down)
	minSize, maxSize float64
	region           []int32 // per-row prefix sums of allowed pixels, (w+1) per row
}

// SetConstraints applies c to all following steps.
func (model *Model) SetConstraints(c Constraints) {
	w, h := model.Target.Bounds().Dx(), model.Target.Bounds().Dy()
	long := float64(max(w, h))
	r := &constraints{
		orient:  c.Focus || c.FixedAngle,
		focus:   c.Focus,
		fx:      c.FocusX * float64(w),
		fy:      c.FocusY * float64(h),
		angle:   -c.Angle,
		minSize: c.MinSize * long,
		maxSize: c.MaxSize * long,
	}
	if c.Region != nil {
		gray := image.NewGray(image.Rect(0, 0, w, h))
		xdraw.ApproxBiLinear.Scale(gray, gray.Rect, c.Region, c.Region.Bounds(), xdraw.Src, nil)
		r.region = make([]int32, (w+1)*h)
		for y := 0; y < h; y++ {
			row := r.region[y*(w+1) : (y+1)*(w+1)]
			for x := 0; x < w; x++ {
				row[x+1] = row[x]
				if gray.Pix[y*gray.Stride+x] >= 128 {
					row[x+1]++
				}
			}
		}
	}
	if !r.orient && r.minSize == 0 && r.maxSize == 0 && r.region == nil {
		r = nil
	}
	for _, worker := range model.Workers {
		worker.constraints = r
	}
}

// direction returns the angle in degrees that the long axis of an oriented
// shape centered at (x, y) must have, if a direction constraint is set.
func (worker *Worker) direction(x, y float64) (float64, bool) {
	c := worker.constraints
	if c == nil || !c.orient {
		return 0, false
	}
	if c.focus {
		return degrees(math.Atan2(c.fy-y, c.fx-x)), true
	}
	return c.angle, true
}

// violation measures how far rasterized lines break the size and region
// constraints; 0 means the shape is allowed.
func (worker *Worker) violation(lines []Scanline) float64 {
	c := worker.constraints
	if c == nil || (c.minSize == 0 && c.maxSize == 0 && c.region == nil) {
		return 0
	}
	if len(lines) == 0 {
		return 1
	}
	var v float64
	if c.minSize > 0 || c.maxSize > 0 {
		x0, y0, x1, y1 := lines[0].X1, lines[0].Y, lines[0].X2, lines[0].Y
		for _, l := range lines[1:] {
			x0, x1 = min(x0, l.X1), max(x1, l.X2)
			y0, y1 = min(y0, l.Y), max(y1, l.Y)
		}
		size := float64(max(x1-x0, y1-y0) + 1)
		if c.minSize > 0 && size < c.minSize {
			v += (c.minSize - size) / c.minSize
		}
		if c.maxSize > 0 && size > c.maxSize {
			v += (size - c.maxSize) / c.maxSize
		}
	}
	if c.region != nil {
		stride := worker.W + 1
		var inside, total int32
		for _, l := range lines {
			row := c.region[l.Y*stride:]
			inside += row[l.X2+1] - row[l.X1]
			total += int32(l.X2 - l.X1 + 1)
		}
		if outside := 1 - float64(inside)/float64(total); outside > regionTolerance {
			v += outside
		}
	}
	return v
}

// Allowed reports whether shape satisfies the worker's constraints.
func (worker *Worker) Allowed(shape Shape) bool {
	if worker.constraints == nil {
		return true
	}
	return worker.violation(shape.Rasterize()) == 0
}
