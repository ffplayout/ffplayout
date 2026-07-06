use std::{
    hint::black_box,
    time::{Duration, Instant},
};

#[path = "../src/compositor/blend.rs"]
mod blend;

const SAMPLE_COUNT: usize = 25;
const SAMPLE_TARGET: Duration = Duration::from_millis(40);

struct Scenario {
    name: &'static str,
    width: usize,
    height: usize,
    opacity: u8,
    alpha_coverage_percent: u8,
}

fn main() {
    let scenarios = [
        Scenario {
            name: "logo_50x50",
            width: 50,
            height: 50,
            opacity: 179,
            alpha_coverage_percent: 80,
        },
        Scenario {
            name: "logo_320x180",
            width: 320,
            height: 180,
            opacity: 179,
            alpha_coverage_percent: 80,
        },
        Scenario {
            name: "text_1280x96",
            width: 1280,
            height: 96,
            opacity: 255,
            alpha_coverage_percent: 25,
        },
        Scenario {
            name: "full_hd_plane",
            width: 1920,
            height: 1080,
            opacity: 220,
            alpha_coverage_percent: 100,
        },
    ];

    println!("overlay alpha blend benchmark");
    println!("times cover one 8-bit plane using the configured row width");

    for scenario in scenarios {
        run_scenario(&scenario);
    }
}

fn run_scenario(scenario: &Scenario) {
    let len = scenario.width * scenario.height;
    let source = generated_bytes(len, 0x243f_6a88);
    let alpha = generated_alpha(len, 0x85a3_08d3, scenario.alpha_coverage_percent);
    let initial = generated_bytes(len, 0x1319_8a2e);

    let mut scalar_result = initial.clone();
    blend::blend_scalar(&mut scalar_result, &source, &alpha, scenario.opacity);

    let scalar = measure(
        &initial,
        &source,
        &alpha,
        scenario.opacity,
        blend::blend_scalar,
    );
    println!(
        "\n{:<18} {:>8} pixels",
        scenario.name,
        scenario.width * scenario.height
    );
    print_result("scalar", scalar, len);

    let mut simd_result = initial.clone();
    blend_rten(
        &mut simd_result,
        &source,
        &alpha,
        scenario.opacity,
        scenario.width,
        scenario.height,
    );
    assert_eq!(
        scalar_result, simd_result,
        "rten-simd output differs from scalar output"
    );

    let simd = measure(
        &initial,
        &source,
        &alpha,
        scenario.opacity,
        |destination, source, alpha, opacity| {
            blend_rten(
                destination,
                source,
                alpha,
                opacity,
                scenario.width,
                scenario.height,
            );
        },
    );
    print_result("rten-simd", simd, len);
    println!(
        "{:<18} {:>8.2}x",
        "speedup",
        scalar.as_secs_f64() / simd.as_secs_f64()
    );
}

fn blend_rten(
    destination: &mut [u8],
    source: &[u8],
    alpha: &[u8],
    opacity: u8,
    width: usize,
    height: usize,
) {
    blend::blend_plane(
        blend::PlaneMut {
            data: destination,
            stride: width,
            x: 0,
            y: 0,
        },
        blend::Plane {
            data: source,
            stride: width,
            x: 0,
            y: 0,
        },
        blend::Plane {
            data: alpha,
            stride: width,
            x: 0,
            y: 0,
        },
        width,
        height,
        opacity,
    );
}

fn measure(
    initial: &[u8],
    source: &[u8],
    alpha: &[u8],
    opacity: u8,
    mut blend: impl FnMut(&mut [u8], &[u8], &[u8], u8),
) -> Duration {
    let mut destination = initial.to_vec();
    let calibration_start = Instant::now();
    let mut calibration_iterations = 0_u64;
    while calibration_start.elapsed() < SAMPLE_TARGET {
        blend(
            black_box(&mut destination),
            black_box(source),
            black_box(alpha),
            black_box(opacity),
        );
        calibration_iterations += 1;
    }
    let iterations = calibration_iterations.max(1);

    let mut samples = Vec::with_capacity(SAMPLE_COUNT);
    for _ in 0..SAMPLE_COUNT {
        destination.copy_from_slice(initial);
        let start = Instant::now();
        for _ in 0..iterations {
            blend(
                black_box(&mut destination),
                black_box(source),
                black_box(alpha),
                black_box(opacity),
            );
        }
        black_box(&destination);
        samples.push(start.elapsed().div_f64(iterations as f64));
    }
    samples.sort_unstable();
    samples[SAMPLE_COUNT / 2]
}

fn print_result(label: &str, elapsed: Duration, pixels: usize) {
    let ns_per_pixel = elapsed.as_secs_f64() * 1_000_000_000.0 / pixels as f64;
    let megapixels_per_second = pixels as f64 / elapsed.as_secs_f64() / 1_000_000.0;
    println!(
        "{label:<18} {:>10.3} us  {:>6.3} ns/pixel  {:>8.1} MP/s",
        elapsed.as_secs_f64() * 1_000_000.0,
        ns_per_pixel,
        megapixels_per_second
    );
}

fn generated_bytes(len: usize, seed: u32) -> Vec<u8> {
    let mut state = seed;
    (0..len)
        .map(|_| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 24) as u8
        })
        .collect()
}

fn generated_alpha(len: usize, seed: u32, coverage_percent: u8) -> Vec<u8> {
    let mut alpha = generated_bytes(len, seed);
    for (index, value) in alpha.iter_mut().enumerate() {
        if index % 100 >= usize::from(coverage_percent) {
            *value = 0;
        }
    }
    alpha
}
