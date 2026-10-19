package primitive

import (
	"fmt"
	"image"
	"image/color"
	"image/gif"
	"io"
	"math"
	"sort"
	"strings"

	"github.com/fogleman/gg"
)

// animationEase shapes the timing of animated output: shape i of n appears
// at fraction (i/n)^animationEase of the animation. Values below 1 spend
// more time on the first shapes, which change the picture the most.
const animationEase = 0.6

// shapeTime returns the fraction (0-1] of the animation at which shape i
// (0-based) of n appears.
func shapeTime(i, n int) float64 {
	return math.Pow(float64(i+1)/float64(n), animationEase)
}

// AnimationCounts returns, for each of the given number of frames, how many
// of the model's shapes are visible. The first frame shows only the
// background and the last frame shows every shape.
func (model *Model) AnimationCounts(frames int) []int {
	n := len(model.Shapes)
	counts := make([]int, frames)
	for k := range counts {
		if frames > 1 {
			t := float64(k) / float64(frames-1)
			counts[k] = int(math.Round(float64(n) * math.Pow(t, 1/animationEase)))
		} else {
			counts[k] = n
		}
	}
	return counts
}

// Animator renders the model's shapes cumulatively at an arbitrary size.
type Animator struct {
	model *Model
	dc    *gg.Context
	scale float64
	drawn int
}

// NewAnimator returns an Animator whose frames fit within maxSide x maxSide.
// If even is set, both dimensions are rounded down to even numbers, as
// most video encoders require.
func (model *Model) NewAnimator(maxSide int, even bool) *Animator {
	s := float64(maxSide) / float64(max(model.Sw, model.Sh))
	w := max(2, int(float64(model.Sw)*s))
	h := max(2, int(float64(model.Sh)*s))
	if even {
		w &^= 1
		h &^= 1
	}
	scale := model.Scale * float64(w) / float64(model.Sw)
	dc := gg.NewContext(w, h)
	dc.Scale(scale, scale)
	dc.Translate(0.5, 0.5)
	dc.SetColor(model.Background.NRGBA())
	dc.Clear()
	return &Animator{model, dc, scale, 0}
}

// Size returns the dimensions of the frames.
func (a *Animator) Size() (int, int) {
	return a.dc.Width(), a.dc.Height()
}

// Frame draws shapes until count are visible and returns the frame. Counts
// must not decrease between calls. The returned image is reused by later
// calls.
func (a *Animator) Frame(count int) *image.RGBA {
	for ; a.drawn < count && a.drawn < len(a.model.Shapes); a.drawn++ {
		c := a.model.Colors[a.drawn]
		a.dc.SetRGBA255(c.R, c.G, c.B, c.A)
		a.model.Shapes[a.drawn].Draw(a.dc, a.scale)
	}
	return a.dc.Image().(*image.RGBA)
}

// AnimatedSVG returns an SVG in which the shapes fade in one after another
// over the given number of seconds, then stay.
func (model *Model) AnimatedSVG(duration float64) string {
	bg := model.Background
	n := len(model.Shapes)
	var lines []string
	lines = append(lines, fmt.Sprintf("<svg xmlns=\"http://www.w3.org/2000/svg\" version=\"1.1\" width=\"%d\" height=\"%d\">", model.Sw, model.Sh))
	lines = append(lines, "<style>.p>*{opacity:0;animation:f .3s ease-out forwards}@keyframes f{to{opacity:1}}@media (prefers-reduced-motion:reduce){.p>*{animation:none;opacity:1}}</style>")
	lines = append(lines, fmt.Sprintf("<rect x=\"0\" y=\"0\" width=\"%d\" height=\"%d\" fill=\"#%02x%02x%02x\" />", model.Sw, model.Sh, bg.R, bg.G, bg.B))
	lines = append(lines, fmt.Sprintf("<g class=\"p\" transform=\"scale(%f) translate(0.5 0.5)\">", model.Scale))
	for i, shape := range model.Shapes {
		c := model.Colors[i]
		attrs := "fill=\"#%02x%02x%02x\" fill-opacity=\"%f\""
		attrs = fmt.Sprintf(attrs, c.R, c.G, c.B, float64(c.A)/255)
		// every shape's SVG starts with "<tag ": add the delay to that element
		delay := fmt.Sprintf(" style=\"animation-delay:%.3fs\"", duration*shapeTime(i, n))
		line := shape.SVG(attrs)
		if j := strings.IndexByte(line, ' '); j > 0 {
			line = line[:j] + delay + line[j:]
		}
		lines = append(lines, line)
	}
	lines = append(lines, "</g>")
	lines = append(lines, "</svg>")
	return strings.Join(lines, "\n")
}

// EncodeGIF writes an animated GIF of shapes being added, fitting within
// maxSide x maxSide. delay is the time between frames and lastDelay the
// time the finished image is held, both in hundredths of a second. It uses
// a palette built from the finished image and stores only the changed
// region of each frame, so it needs no external tools.
func (model *Model) EncodeGIF(w io.Writer, maxSide, frames, delay, lastDelay int) error {
	counts := model.AnimationCounts(max(frames, 2))
	a := model.NewAnimator(maxSide, false)
	width, height := a.Size()

	// palette from the finished image; index 255 is reserved for transparency
	final := model.NewAnimator(maxSide, false).Frame(len(model.Shapes))
	pal := quantize(final, 255)
	transparent := uint8(len(pal))
	pal = append(pal, color.RGBA{})
	lookup := newPaletteLookup(pal[:transparent])

	g := &gif.GIF{
		Config: image.Config{ColorModel: pal, Width: width, Height: height},
	}
	prev := make([]uint8, width*height)
	cur := make([]uint8, width*height)
	for k, count := range counts {
		if k > 0 && count == counts[k-1] {
			// nothing new to show: lengthen the previous frame instead
			g.Delay[len(g.Delay)-1] += delay
			continue
		}
		im := a.Frame(count)
		for y := 0; y < height; y++ {
			row := im.Pix[y*im.Stride : y*im.Stride+width*4]
			for x := 0; x < width; x++ {
				cur[y*width+x] = lookup.index(row[x*4], row[x*4+1], row[x*4+2])
			}
		}
		rect := image.Rect(0, 0, width, height)
		if k > 0 {
			rect = changedRect(prev, cur, width, height)
			if rect.Empty() {
				g.Delay[len(g.Delay)-1] += delay
				continue
			}
		}
		frame := image.NewPaletted(rect, pal)
		for y := rect.Min.Y; y < rect.Max.Y; y++ {
			for x := rect.Min.X; x < rect.Max.X; x++ {
				i := y*width + x
				if k > 0 && cur[i] == prev[i] {
					frame.Pix[frame.PixOffset(x, y)] = transparent
				} else {
					frame.Pix[frame.PixOffset(x, y)] = cur[i]
				}
			}
		}
		g.Image = append(g.Image, frame)
		g.Delay = append(g.Delay, delay)
		g.Disposal = append(g.Disposal, gif.DisposalNone)
		prev, cur = cur, prev
	}
	g.Delay[len(g.Delay)-1] = lastDelay
	return gif.EncodeAll(w, g)
}

// changedRect returns the bounding box of the pixels that differ.
func changedRect(a, b []uint8, width, height int) image.Rectangle {
	x0, y0, x1, y1 := width, height, -1, -1
	for y := 0; y < height; y++ {
		for x := 0; x < width; x++ {
			if a[y*width+x] != b[y*width+x] {
				x0, x1 = min(x0, x), max(x1, x)
				y0, y1 = min(y0, y), max(y1, y)
			}
		}
	}
	if x1 < 0 {
		return image.Rectangle{}
	}
	return image.Rect(x0, y0, x1+1, y1+1)
}

// quantize builds a palette of at most n colors for im using median cut
// over a 5-bit-per-channel histogram.
func quantize(im *image.RGBA, n int) color.Palette {
	type bin struct {
		count      int
		r, g, b    int   // sums of the exact colors in this bin
		cr, cg, cb uint8 // 5-bit coordinates
	}
	bins := map[int]*bin{}
	b := im.Bounds()
	for y := b.Min.Y; y < b.Max.Y; y++ {
		for x := b.Min.X; x < b.Max.X; x++ {
			i := im.PixOffset(x, y)
			r, g, bl := im.Pix[i], im.Pix[i+1], im.Pix[i+2]
			key := int(r>>3)<<10 | int(g>>3)<<5 | int(bl>>3)
			e := bins[key]
			if e == nil {
				e = &bin{cr: r >> 3, cg: g >> 3, cb: bl >> 3}
				bins[key] = e
			}
			e.count++
			e.r += int(r)
			e.g += int(g)
			e.b += int(bl)
		}
	}
	all := make([]*bin, 0, len(bins))
	for _, e := range bins {
		all = append(all, e)
	}
	channel := func(e *bin, c int) uint8 {
		switch c {
		case 0:
			return e.cr
		case 1:
			return e.cg
		}
		return e.cb
	}
	// boxes are split along their widest channel at the weighted median
	boxes := [][]*bin{all}
	for len(boxes) < n {
		best, bestRange, bestChannel := -1, 0, 0
		for i, box := range boxes {
			if len(box) < 2 {
				continue
			}
			for c := 0; c < 3; c++ {
				lo, hi := uint8(255), uint8(0)
				for _, e := range box {
					v := channel(e, c)
					lo, hi = min(lo, v), max(hi, v)
				}
				if int(hi-lo) > bestRange {
					best, bestRange, bestChannel = i, int(hi-lo), c
				}
			}
		}
		if best < 0 {
			break
		}
		box := boxes[best]
		sort.Slice(box, func(i, j int) bool {
			return channel(box[i], bestChannel) < channel(box[j], bestChannel)
		})
		total := 0
		for _, e := range box {
			total += e.count
		}
		split, acc := 1, 0
		for i, e := range box[:len(box)-1] {
			acc += e.count
			split = i + 1
			if acc*2 >= total {
				break
			}
		}
		boxes[best] = box[:split]
		boxes = append(boxes, box[split:])
	}
	pal := make(color.Palette, 0, len(boxes))
	for _, box := range boxes {
		var count, r, g, bl int
		for _, e := range box {
			count += e.count
			r += e.r
			g += e.g
			bl += e.b
		}
		pal = append(pal, color.RGBA{uint8(r / count), uint8(g / count), uint8(bl / count), 255})
	}
	return pal
}

// paletteLookup maps colors to their nearest palette index through a
// 5-bit-per-channel table, which is much faster than searching per pixel.
type paletteLookup struct {
	table [32 * 32 * 32]uint8
}

func newPaletteLookup(pal color.Palette) *paletteLookup {
	l := &paletteLookup{}
	rgb := make([][3]int, len(pal))
	for i, c := range pal {
		r, g, b, _ := c.RGBA()
		rgb[i] = [3]int{int(r >> 8), int(g >> 8), int(b >> 8)}
	}
	for key := range l.table {
		r := (key>>10)<<3 | 4
		g := (key>>5&31)<<3 | 4
		b := (key&31)<<3 | 4
		best, bestDist := 0, math.MaxInt
		for i, c := range rgb {
			dr, dg, db := r-c[0], g-c[1], b-c[2]
			if d := 2*dr*dr + 4*dg*dg + 3*db*db; d < bestDist {
				best, bestDist = i, d
			}
		}
		l.table[key] = uint8(best)
	}
	return l
}

func (l *paletteLookup) index(r, g, b uint8) uint8 {
	return l.table[int(r>>3)<<10|int(g>>3)<<5|int(b>>3)]
}
