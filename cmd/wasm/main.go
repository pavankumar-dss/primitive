//go:build js && wasm

// Command wasm exposes the primitive model to JavaScript for the browser
// version of primitive (see the site directory). The page decodes and
// resizes the input image itself and passes raw RGBA pixels in.
package main

import (
	"bytes"
	"image"
	"image/png"
	"math/rand"
	"syscall/js"

	"github.com/pavankumar-dss/primitive/primitive"
)

var model *primitive.Model

// primitiveInit(rgba Uint8Array, width, height, outputSize, background,
// constraints, seed) creates a new model. background is a hex color, or ""
// for the average color of the image. constraints is optional: {focus:
// [x, y], angle, minSize, maxSize, region: Uint8Array of width*height
// bytes}, all fields optional. seed, also optional, seeds the random search,
// so that models in different Web Workers don't search the same way.
// Returns the starting score.
func initModel(this js.Value, args []js.Value) any {
	w, h := args[1].Int(), args[2].Int()
	im := image.NewRGBA(image.Rect(0, 0, w, h))
	js.CopyBytesToGo(im.Pix, args[0])
	var bg primitive.Color
	if hex := args[4].String(); hex == "" {
		bg = primitive.MakeColor(primitive.AverageImageColor(im))
	} else {
		bg = primitive.MakeHexColor(hex)
	}
	// a single worker: goroutines don't run in parallel under js/wasm
	model = primitive.NewModel(im, bg, args[3].Int(), 1)
	if len(args) > 5 && args[5].Type() == js.TypeObject {
		model.SetConstraints(parseConstraints(args[5], w, h))
	}
	if len(args) > 6 && args[6].Type() == js.TypeNumber {
		model.Workers[0].Rnd = rand.New(rand.NewSource(int64(args[6].Float())))
	}
	best = nil
	return model.Score
}

func parseConstraints(o js.Value, w, h int) primitive.Constraints {
	var c primitive.Constraints
	if f := o.Get("focus"); f.Truthy() {
		c.Focus, c.FocusX, c.FocusY = true, f.Index(0).Float(), f.Index(1).Float()
	}
	if a := o.Get("angle"); a.Type() == js.TypeNumber {
		c.FixedAngle, c.Angle = true, a.Float()
	}
	if v := o.Get("minSize"); v.Type() == js.TypeNumber {
		c.MinSize = v.Float()
	}
	if v := o.Get("maxSize"); v.Type() == js.TypeNumber {
		c.MaxSize = v.Float()
	}
	if r := o.Get("region"); r.Truthy() {
		region := image.NewGray(image.Rect(0, 0, w, h))
		js.CopyBytesToGo(region.Pix, r)
		c.Region = region
	}
	return c
}

// primitiveShapes() returns the number of shapes in the model.
func shapes(this js.Value, args []js.Value) any {
	if model == nil {
		return 0
	}
	return len(model.Shapes)
}

// primitiveStep(mode, alpha, repeat) adds one shape (plus repeat extra
// shapes) and returns the new score.
func step(this js.Value, args []js.Value) any {
	model.Step(primitive.ShapeType(args[0].Int()), args[1].Int(), args[2].Int())
	return model.Score
}

// The browser spreads each step over several models in separate Web
// Workers: each runs primitiveSearch, the one that found the best shape
// runs primitiveCommit, and the rest replay its shapes with primitiveApply.

var best *primitive.State

// primitiveSearch(mode, alpha, searches, fresh) looks for a shape without
// adding it, keeping the best found since the last call with fresh set or
// the last commit, and returns that shape's energy (lower is better).
func search(this js.Value, args []js.Value) any {
	s := model.Search(primitive.ShapeType(args[0].Int()), args[1].Int(), args[2].Int())
	if best == nil || args[3].Truthy() || s.Energy() < best.Energy() {
		best = s
	}
	return best.Energy()
}

// primitiveCommit(repeat) adds the shape found by the last search, plus up
// to repeat more, and returns them as [[alpha, ...encoded shape], ...].
func commit(this js.Value, args []js.Value) any {
	var out []any
	for _, s := range model.Commit(best, args[0].Int()) {
		v := []any{s.Alpha}
		for _, x := range primitive.EncodeShape(s.Shape) {
			v = append(v, x)
		}
		out = append(out, v)
	}
	best = nil
	return out
}

// primitiveApply(shapes) adds shapes returned by primitiveCommit in another
// model and returns the new score.
func apply(this js.Value, args []js.Value) any {
	list := args[0]
	for i := 0; i < list.Length(); i++ {
		item := list.Index(i)
		v := make([]float64, item.Length()-1)
		for k := range v {
			v[k] = item.Index(k + 1).Float()
		}
		shape, err := primitive.DecodeShape(model.Workers[0], v)
		if err != nil {
			panic(err)
		}
		model.Add(shape, item.Index(0).Int())
	}
	return model.Score
}

// primitiveSVG() returns the current model as an SVG document.
func svg(this js.Value, args []js.Value) any {
	if model == nil {
		return js.Null()
	}
	return model.SVG()
}

// primitivePNG() returns the current model rendered at the output size as
// PNG bytes.
func pngBytes(this js.Value, args []js.Value) any {
	if model == nil {
		return js.Null()
	}
	var buf bytes.Buffer
	if err := png.Encode(&buf, model.Context.Image()); err != nil {
		return js.Null()
	}
	return toJS(buf.Bytes())
}

// primitiveAnimatedSVG(duration) returns an SVG in which the shapes fade
// in over duration seconds.
func animatedSVG(this js.Value, args []js.Value) any {
	if model == nil {
		return js.Null()
	}
	return model.AnimatedSVG(args[0].Float())
}

// primitiveGIF(maxSide, frames, delay, lastDelay) returns an animated GIF;
// delays are in hundredths of a second.
func gifBytes(this js.Value, args []js.Value) any {
	if model == nil {
		return js.Null()
	}
	var buf bytes.Buffer
	if err := model.EncodeGIF(&buf, args[0].Int(), args[1].Int(), args[2].Int(), args[3].Int()); err != nil {
		return js.Null()
	}
	return toJS(buf.Bytes())
}

var (
	animator *primitive.Animator
	counts   []int
)

// primitiveAnimStart(maxSide, frames) prepares to render video frames with
// even dimensions and returns {width, height}.
func animStart(this js.Value, args []js.Value) any {
	if model == nil {
		return js.Null()
	}
	animator = model.NewAnimator(args[0].Int(), true)
	counts = model.AnimationCounts(args[1].Int())
	w, h := animator.Size()
	return map[string]any{"width": w, "height": h}
}

// primitiveAnimFrame(k) returns frame k as RGBA bytes. Frames must be
// requested in order.
func animFrame(this js.Value, args []js.Value) any {
	return toJS(animator.Frame(counts[args[0].Int()]).Pix)
}

func toJS(b []byte) js.Value {
	out := js.Global().Get("Uint8Array").New(len(b))
	js.CopyBytesToJS(out, b)
	return out
}

func main() {
	js.Global().Set("primitiveInit", js.FuncOf(initModel))
	js.Global().Set("primitiveStep", js.FuncOf(step))
	js.Global().Set("primitiveShapes", js.FuncOf(shapes))
	js.Global().Set("primitiveSearch", js.FuncOf(search))
	js.Global().Set("primitiveCommit", js.FuncOf(commit))
	js.Global().Set("primitiveApply", js.FuncOf(apply))
	js.Global().Set("primitiveSVG", js.FuncOf(svg))
	js.Global().Set("primitivePNG", js.FuncOf(pngBytes))
	js.Global().Set("primitiveAnimatedSVG", js.FuncOf(animatedSVG))
	js.Global().Set("primitiveGIF", js.FuncOf(gifBytes))
	js.Global().Set("primitiveAnimStart", js.FuncOf(animStart))
	js.Global().Set("primitiveAnimFrame", js.FuncOf(animFrame))
	select {}
}
