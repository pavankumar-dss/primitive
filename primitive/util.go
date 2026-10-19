package primitive

import (
	"fmt"
	"image"
	"image/color"
	"image/color/palette"
	"image/draw"
	"image/gif"
	"image/jpeg"
	"image/png"
	"math"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"

	xdraw "golang.org/x/image/draw"
)

func LoadImage(path string) (image.Image, error) {
	if path == "-" {
		im, _, err := image.Decode(os.Stdin)
		return im, err
	} else {
		file, err := os.Open(path)
		if err != nil {
			return nil, err
		}
		defer file.Close()
		im, _, err := image.Decode(file)
		return im, err
	}
}

// Thumbnail scales im down to fit within size x size, preserving its aspect
// ratio. Images that already fit are returned unchanged.
func Thumbnail(im image.Image, size int) image.Image {
	b := im.Bounds()
	w, h := b.Dx(), b.Dy()
	if w <= size && h <= size {
		return im
	}
	scale := math.Min(float64(size)/float64(w), float64(size)/float64(h))
	nw := max(1, int(math.Round(float64(w)*scale)))
	nh := max(1, int(math.Round(float64(h)*scale)))
	dst := image.NewRGBA(image.Rect(0, 0, nw, nh))
	xdraw.BiLinear.Scale(dst, dst.Bounds(), im, b, xdraw.Src, nil)
	return dst
}

func SaveFile(path, contents string) error {
	if path == "-" {
		_, err := fmt.Fprint(os.Stdout, contents)
		return err
	} else {
		file, err := os.Create(path)
		if err != nil {
			return err
		}
		defer file.Close()
		_, err = file.WriteString(contents)
		return err
	}
}

func SavePNG(path string, im image.Image) error {
	file, err := os.Create(path)
	if err != nil {
		return err
	}
	defer file.Close()
	return png.Encode(file, im)
}

func SaveJPG(path string, im image.Image, quality int) error {
	file, err := os.Create(path)
	if err != nil {
		return err
	}
	defer file.Close()
	return jpeg.Encode(file, im, &jpeg.Options{Quality: quality})
}

func SaveGIF(path string, frames []image.Image, delay, lastDelay int) error {
	g := gif.GIF{}
	for i, src := range frames {
		dst := image.NewPaletted(src.Bounds(), palette.Plan9)
		draw.Draw(dst, dst.Rect, src, image.Point{}, draw.Src)
		g.Image = append(g.Image, dst)
		if i == len(frames)-1 {
			g.Delay = append(g.Delay, lastDelay)
		} else {
			g.Delay = append(g.Delay, delay)
		}
	}
	file, err := os.Create(path)
	if err != nil {
		return err
	}
	defer file.Close()
	return gif.EncodeAll(file, &g)
}

// SaveAnimatedGIF writes an animated GIF of the model's shapes being added;
// see Model.EncodeGIF.
func SaveAnimatedGIF(path string, model *Model, maxSide, frames, delay, lastDelay int) error {
	file, err := os.Create(path)
	if err != nil {
		return err
	}
	defer file.Close()
	return model.EncodeGIF(file, maxSide, frames, delay, lastDelay)
}

func SaveGIFImageMagick(path string, frames []image.Image, delay, lastDelay int) error {
	dir, err := os.MkdirTemp("", "primitive")
	if err != nil {
		return err
	}
	defer os.RemoveAll(dir)
	for i, im := range frames {
		path := filepath.Join(dir, fmt.Sprintf("%06d.png", i))
		if err := SavePNG(path, im); err != nil {
			return err
		}
	}
	args := []string{
		"-loop", "0",
		"-delay", fmt.Sprint(delay),
		filepath.Join(dir, "*.png"),
		"-delay", fmt.Sprint(lastDelay - delay),
		filepath.Join(dir, fmt.Sprintf("%06d.png", len(frames)-1)),
		path,
	}
	// ImageMagick 7 ships "magick"; "convert" is the legacy (IM6) name
	// and collides with a system tool on Windows.
	name := "magick"
	if _, err := exec.LookPath(name); err != nil {
		if runtime.GOOS == "windows" {
			return fmt.Errorf("gif output requires ImageMagick 7 (magick) on PATH")
		}
		name = "convert"
	}
	cmd := exec.Command(name, args...)
	cmd.Stderr = os.Stderr
	return cmd.Run()
}

func NumberString(x float64) string {
	suffixes := []string{"", "k", "M", "G"}
	for _, suffix := range suffixes {
		if x < 1000 {
			return fmt.Sprintf("%.1f%s", x, suffix)
		}
		x /= 1000
	}
	return fmt.Sprintf("%.1f%s", x, "T")
}

func radians(degrees float64) float64 {
	return degrees * math.Pi / 180
}

func degrees(radians float64) float64 {
	return radians * 180 / math.Pi
}

func clamp(x, lo, hi float64) float64 {
	if x < lo {
		return lo
	}
	if x > hi {
		return hi
	}
	return x
}

func clampInt(x, lo, hi int) int {
	if x < lo {
		return lo
	}
	if x > hi {
		return hi
	}
	return x
}

func minInt(a, b int) int {
	if a < b {
		return a
	}
	return b
}

func maxInt(a, b int) int {
	if a > b {
		return a
	}
	return b
}

func rotate(x, y, theta float64) (rx, ry float64) {
	rx = x*math.Cos(theta) - y*math.Sin(theta)
	ry = x*math.Sin(theta) + y*math.Cos(theta)
	return
}

func imageToRGBA(src image.Image) *image.RGBA {
	dst := image.NewRGBA(src.Bounds())
	draw.Draw(dst, dst.Rect, src, image.Point{}, draw.Src)
	return dst
}

func copyRGBA(src *image.RGBA) *image.RGBA {
	dst := image.NewRGBA(src.Bounds())
	copy(dst.Pix, src.Pix)
	return dst
}

func uniformRGBA(r image.Rectangle, c color.Color) *image.RGBA {
	im := image.NewRGBA(r)
	draw.Draw(im, im.Bounds(), &image.Uniform{c}, image.Point{}, draw.Src)
	return im
}

func AverageImageColor(im image.Image) color.NRGBA {
	rgba := imageToRGBA(im)
	size := rgba.Bounds().Size()
	w, h := size.X, size.Y
	var r, g, b int
	for y := 0; y < h; y++ {
		for x := 0; x < w; x++ {
			c := rgba.RGBAAt(x, y)
			r += int(c.R)
			g += int(c.G)
			b += int(c.B)
		}
	}
	r /= w * h
	g /= w * h
	b /= w * h
	return color.NRGBA{uint8(r), uint8(g), uint8(b), 255}
}
