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

// primitiveInit(rgba Uint8Array, width, height, outputSize, background)
// creates a new model. background is a hex color, or "" for the average
// color of the image. Returns the starting score.
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
	return model.Score
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
	out := js.Global().Get("Uint8Array").New(buf.Len())
	js.CopyBytesToJS(out, buf.Bytes())
	return out
}

func main() {
	js.Global().Set("primitiveInit", js.FuncOf(initModel))
	js.Global().Set("primitiveStep", js.FuncOf(step))
	js.Global().Set("primitiveSVG", js.FuncOf(svg))
	js.Global().Set("primitivePNG", js.FuncOf(pngBytes))
	select {}
}
