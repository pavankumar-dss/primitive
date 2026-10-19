//go:build js && wasm

// Command wasm exposes the primitive model to JavaScript for the browser
// version of primitive (see the site directory). The page decodes and
// resizes the input image itself and passes raw RGBA pixels in.
package main

import (
	"bytes"
	"image"
	"image/png"
	"syscall/js"

	"github.com/pavankumar-dss/primitive/primitive"
)

var model *primitive.Model

// primitiveInit(rgba Uint8Array, width, height, outputSize, background,
// constraints) creates a new model. background is a hex color, or "" for
// the average color of the image. constraints is optional: {focus: [x, y],
// angle, minSize, maxSize, region: Uint8Array of width*height bytes}, all
// fields optional. Returns the starting score.
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
	js.Global().Set("primitiveSVG", js.FuncOf(svg))
	js.Global().Set("primitivePNG", js.FuncOf(pngBytes))
	js.Global().Set("primitiveAnimatedSVG", js.FuncOf(animatedSVG))
	js.Global().Set("primitiveGIF", js.FuncOf(gifBytes))
	js.Global().Set("primitiveAnimStart", js.FuncOf(animStart))
	js.Global().Set("primitiveAnimFrame", js.FuncOf(animFrame))
	select {}
}
