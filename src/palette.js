// A dependency-free palette with fixed OKLCH lightness and chroma, returning sRGB hex.
// Oklab conversion matrices: Björn Ottosson's public-domain reference,
// https://bottosson.github.io/posts/oklab/ (2021-01-25 matrices).
// Only hue changes, in rainbow order. These common-gamut values fit the entire
// hue circle without clipping channels or reducing chroma for individual colours.
// L=0.7501536182, C=0.1275292193 is the full-circle sRGB chroma maximum;
// the values below retain a little numerical headroom. Conversion to
// 8-bit hex introduces small rounding differences and, in large palettes,
// repeated colours. No finite colour space can supply unlimited unique colours.
(() => {
    'use strict';
    // Captured only inside the private bundled runner; standalone use needs no host.
    const reportProgress = typeof checkpointHost === 'function' ? checkpointHost : () => {};
    const integer = Number.isSafeInteger;
    const push = Function.prototype.call.bind(Array.prototype.push);
    const sort = Function.prototype.call.bind(Array.prototype.sort);
    const sliceString = Function.prototype.call.bind(String.prototype.slice);
    const parse = Number.parseInt;
    const ErrorType = Error;
    const math = Math;
    const lightness = 0.75015;
    const chroma = 0.1275;
    // The sRGB red axis (green = blue) on this fixed-lightness/chroma circle.
    const startingHue = 20.52934746176819 / 360;

    function linearRgb(a, b) {
        const l = (lightness + 0.3963377774 * a + 0.2158037573 * b) ** 3;
        const m = (lightness - 0.1055613458 * a - 0.0638541728 * b) ** 3;
        const s = (lightness - 0.0894841775 * a - 1.2914855480 * b) ** 3;
        return [
            4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
            -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
            -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s
        ];
    }

    function colourAt(index, count) {
        // Every size spans the whole rainbow, so different sizes need not share
        // a prefix. The final entry approaches red without repeating the endpoint.
        const angle = (startingHue + index / count) * math.PI * 2;
        const rgb = linearRgb(chroma * math.cos(angle), chroma * math.sin(angle));
        const digits = '0123456789abcdef';
        let hex = '#';
        for (let channel = 0; channel < 3; channel++) {
            const linear = rgb[channel];
            const srgb = linear <= 0.0031308 ? 12.92 * linear :
                1.055 * linear ** (1 / 2.4) - 0.055;
            const byte = math.round(srgb * 255);
            hex += digits[byte >>> 4] + digits[byte & 15];
        }
        return hex;
    }

    function rgbHue(hex) {
        const packed = parse(sliceString(hex, 1), 16);
        const r = packed >>> 16, g = (packed >>> 8) & 255, b = packed & 255;
        const maximum = math.max(r, g, b);
        const delta = maximum - math.min(r, g, b);
        if (delta === 0) return 0;
        let hue;
        if (maximum === r) hue = (g - b) / delta;
        else if (maximum === g) hue = (b - r) / delta + 2;
        else hue = (r - g) / delta + 4;
        return hue < 0 ? hue + 6 : hue;
    }

    return function palette(count) {
        if (!integer(count) || count < 0) {
            throw new ErrorType('palette(count) requires a nonnegative safe integer');
        }
        const chosen = [];
        while (chosen.length < count) {
            if ((chosen.length & 255) === 0) reportProgress();
            push(chosen, colourAt(chosen.length, count));
        }
        // Rounding to 8-bit channels can make nearby hues swap order. Sort the
        // final hex colours so even very large palettes retain rainbow order.
        let comparisons = 0;
        sort(chosen, (a, b) => {
            if ((comparisons++ & 255) === 0) reportProgress();
            return rgbHue(a) - rgbHue(b);
        });
        // Each request owns its array. Retaining many differently sized palettes
        // in a private cache would unnecessarily duplicate experiment memory.
        return chosen;
    };
})()
