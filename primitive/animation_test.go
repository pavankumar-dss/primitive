package primitive

import (
	"bytes"
	"image"
	"image/color"
	"image/draw"
	"image/gif"
	"strings"
	"testing"
)

func testModel(t *testing.T, shapes int) *Model {
	t.Helper()
	im := image.NewRGBA(image.Rect(0, 0, 64, 48))
	for y := 0; y < 48; y++ {
		for x := 0; x < 64; x++ {
			im.Set(x, y, color.RGBA{uint8(x * 4), uint8(y * 5), 128, 255})
		}
	}
	model := NewModel(im, MakeColor(AverageImageColor(im)), 128, 1)
	for i := 0; i < shapes; i++ {
		model.Step(ShapeTypeTriangle, 128, 0)
	}
	return model
}

func TestAnimationCounts(t *testing.T) {
	model := testModel(t, 10)
	counts := model.AnimationCounts(20)
	if counts[0] != 0 || counts[len(counts)-1] != 10 {
		t.Fatalf("counts should run from 0 to 10, got %v", counts)
	}
	for i := 1; i < len(counts); i++ {
		if counts[i] < counts[i-1] {
			t.Fatalf("counts decrease: %v", counts)
		}
	}
}

func TestEncodeGIF(t *testing.T) {
	model := testModel(t, 20)
	var buf bytes.Buffer
	if err := model.EncodeGIF(&buf, 128, 30, 5, 200); err != nil {
		t.Fatal(err)
	}
	g, err := gif.DecodeAll(&buf)
	if err != nil {
		t.Fatal(err)
	}
	if len(g.Image) < 2 || g.Delay[len(g.Delay)-1] != 200 {
		t.Fatalf("got %d frames, last delay %d", len(g.Image), g.Delay[len(g.Delay)-1])
	}
	// composite the frames and compare with a direct render of all shapes
	w, h := g.Config.Width, g.Config.Height
	canvas := image.NewRGBA(image.Rect(0, 0, w, h))
	for _, frame := range g.Image {
		draw.Draw(canvas, frame.Bounds(), frame, frame.Bounds().Min, draw.Over)
	}
	want := model.NewAnimator(128, false).Frame(len(model.Shapes))
	var diff float64
	for i := 0; i < len(want.Pix); i += 4 {
		for c := 0; c < 3; c++ {
			d := float64(want.Pix[i+c]) - float64(canvas.Pix[i+c])
			diff += d * d
		}
	}
	if rmse := diff / float64(w*h*3); rmse > 100 {
		t.Fatalf("final GIF frame differs from the model: mean squared error %.1f", rmse)
	}
}

func TestAnimatedSVG(t *testing.T) {
	model := testModel(t, 5)
	svg := model.AnimatedSVG(4)
	if n := strings.Count(svg, "animation-delay:"); n != 5 {
		t.Fatalf("expected 5 animated shapes, got %d", n)
	}
	if !strings.Contains(svg, "animation-delay:4.000s") {
		t.Fatal("last shape should appear at the end of the animation")
	}
}
