// A dependency-free categorical palette in Oklab / OKLCH, returning sRGB hex.
// Oklab conversion matrices: Björn Ottosson's public-domain reference,
// https://bottosson.github.io/posts/oklab/ (2021-01-25 matrices).
// The first 128 colours maximise the nearest Oklab distance among a fixed set
// of vivid candidates. Larger palettes continue with low-discrepancy OKLCH
// sampling. Hex values remain unique, but thousands cannot be visually distinct.
(() => {
    'use strict';
    const integer = Number.isInteger;
    const slice = Function.prototype.call.bind(Array.prototype.slice);
    const has = Function.prototype.call.bind(Set.prototype.has);
    const add = Function.prototype.call.bind(Set.prototype.add);
    const ErrorType = Error;
    const math = Math;
    const chosen = [];
    const used = new Set();
    const candidates = [];
    let extension = 0;

    function linearRgb(lightness, a, b) {
        const l = (lightness + 0.3963377774 * a + 0.2158037573 * b) ** 3;
        const m = (lightness - 0.1055613458 * a - 0.0638541728 * b) ** 3;
        const s = (lightness - 0.0894841775 * a - 1.2914855480 * b) ** 3;
        return [
            4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
            -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
            -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s
        ];
    }

    function inGamut(rgb) {
        return rgb[0] >= 0 && rgb[0] <= 1 &&
            rgb[1] >= 0 && rgb[1] <= 1 && rgb[2] >= 0 && rgb[2] <= 1;
    }

    function vivid(lightness, hue, saturation = 0.94) {
        const angle = hue * math.PI * 2;
        const x = math.cos(angle), y = math.sin(angle);
        // Reduce chroma at constant lightness and hue to fit sRGB, rather than
        // clipping channels, which would distort the intended perceptual hue.
        let low = 0, high = 0.34;
        for (let step = 0; step < 14; step++) {
            const middle = (low + high) / 2;
            if (inGamut(linearRgb(lightness, middle * x, middle * y))) low = middle;
            else high = middle;
        }
        const chroma = low * saturation;
        const a = chroma * x, b = chroma * y;
        const rgb = linearRgb(lightness, a, b);
        const digits = '0123456789abcdef';
        let hex = '#';
        for (let channel = 0; channel < 3; channel++) {
            const linear = rgb[channel];
            const srgb = linear <= 0.0031308 ? 12.92 * linear :
                1.055 * linear ** (1 / 2.4) - 0.055;
            const byte = math.round(math.max(0, math.min(1, srgb)) * 255);
            hex += digits[byte >>> 4] + digits[byte & 15];
        }
        return { lightness, a, b, hex, nearest: Infinity };
    }

    function initialise() {
        if (candidates.length) return;
        // Seed with a bright blue. Every prefix is stable when count increases.
        candidates[0] = vivid(0.70, 255 / 360);
        const lightnesses = [0.64, 0.70, 0.76, 0.82];
        for (let hue = 0; hue < 180; hue++) {
            // Select the lightness with greatest in-gamut chroma at this hue.
            // A fixed lightness can make yellow muddy or pink pastel even at
            // maximum saturation. This keeps the candidate pool vivid while
            // letting each hue find its most colourful usable brightness.
            let mostChromatic;
            for (let level = 0; level < lightnesses.length; level++) {
                const colour = vivid(lightnesses[level], hue / 180);
                if (!mostChromatic || colour.a ** 2 + colour.b ** 2 >
                    mostChromatic.a ** 2 + mostChromatic.b ** 2) {
                    mostChromatic = colour;
                }
            }
            candidates[candidates.length] = mostChromatic;
        }
    }

    function chooseNext() {
        let best;
        for (let index = 0; index < candidates.length; index++) {
            const candidate = candidates[index];
            if (!has(used, candidate.hex) && (!best || candidate.nearest > best.nearest)) {
                best = candidate;
            }
        }
        chosen[chosen.length] = best.hex;
        add(used, best.hex);
        for (let index = 0; index < candidates.length; index++) {
            const candidate = candidates[index];
            const dl = candidate.lightness - best.lightness;
            const da = candidate.a - best.a;
            const db = candidate.b - best.b;
            candidate.nearest = math.min(candidate.nearest, dl * dl + da * da + db * db);
        }
    }

    return function palette(count) {
        if (!integer(count) || count < 0 || count > 8192) {
            throw new ErrorType('palette(count) requires an integer from 0 to 8192');
        }
        if (count === 0) return [];
        initialise();
        while (chosen.length < math.min(count, 128)) chooseNext();
        while (chosen.length < count) {
            if (extension >= 32768) throw new ErrorType('Palette candidate budget exhausted');
            extension += 1;
            // Different irrational steps spread hue, lightness and saturation
            // without using or changing the experiment's seeded random stream.
            const hue = (extension * 0.6180339887498949 + 255 / 360) % 1;
            const lightness = 0.64 + 0.18 * ((extension * 0.4142135623730951) % 1);
            const saturation = 0.82 + 0.16 * ((extension * 0.7320508075688772) % 1);
            const colour = vivid(lightness, hue, saturation);
            if (!has(used, colour.hex)) {
                chosen[chosen.length] = colour.hex;
                add(used, colour.hex);
            }
        }
        // Caller edits must never affect a later palette request.
        return slice(chosen, 0, count);
    };
})()
