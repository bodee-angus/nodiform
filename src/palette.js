// A dependency-free palette with fixed OKLCH lightness and chroma, returning sRGB hex.
// Oklab conversion matrices: Björn Ottosson's public-domain reference,
// https://bottosson.github.io/posts/oklab/ (2021-01-25 matrices).
// Only hue changes. These common-gamut values fit the entire hue circle without
// clipping channels or reducing chroma for individual colours. Conversion to
// 8-bit hex introduces small rounding differences and, in large palettes,
// repeated colours. No finite colour space can supply unlimited unique colours.
(() => {
    'use strict';
    // Captured only inside the private bundled runner; standalone use needs no host.
    const reportProgress = typeof checkpointHost === 'function' ? checkpointHost : () => {};
    const integer = Number.isSafeInteger;
    const slice = Function.prototype.call.bind(Array.prototype.slice);
    const push = Function.prototype.call.bind(Array.prototype.push);
    const ErrorType = Error;
    const math = Math;
    const chosen = [];
    const lightness = 0.75;
    const chroma = 0.127;
    const hueStep = 0.6180339887498949;
    const startingHue = 255 / 360;

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

    function colourAt(index) {
        // Golden-angle spacing spreads neighbouring entries around the circle.
        // The index alone determines hue, so growing a palette preserves its prefix.
        const angle = ((startingHue + index * hueStep) % 1) * math.PI * 2;
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

    return function palette(count) {
        if (!integer(count) || count < 0) {
            throw new ErrorType('palette(count) requires a nonnegative safe integer');
        }
        while (chosen.length < count) {
            if ((chosen.length & 255) === 0) reportProgress();
            push(chosen, colourAt(chosen.length));
        }
        // Caller edits must never affect a later palette request.
        return slice(chosen, 0, count);
    };
})()
