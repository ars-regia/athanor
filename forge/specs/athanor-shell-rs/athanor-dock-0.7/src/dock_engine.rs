use serde::{Deserialize, Serialize};

/// Mode of the Dock presentation:
/// - Fashion: Floating centered pill with glassmorphism and fisheye magnification
/// - Efficient: Full-width taskbar spanning across the bottom of the screen
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum DockMode {
    #[default]
    Fashion,
    Efficient,
}


impl DockMode {
    pub fn toggle(&self) -> Self {
        match self {
            DockMode::Fashion => DockMode::Efficient,
            DockMode::Efficient => DockMode::Fashion,
        }
    }

    pub fn is_fashion(&self) -> bool {
        matches!(self, DockMode::Fashion)
    }

    pub fn is_efficient(&self) -> bool {
        matches!(self, DockMode::Efficient)
    }
}

/// Parameters for the non-linear fisheye icon magnification algorithm.
#[derive(Debug, Clone, PartialEq)]
pub struct FisheyeConfig {
    pub base_size: f64,
    pub max_scale: f64,
    pub radius: f64,
    pub curve_exponent: f64,
}

impl Default for FisheyeConfig {
    fn default() -> Self {
        Self {
            base_size: 44.0,
            max_scale: 1.45,
            radius: 160.0,
            curve_exponent: 1.5,
        }
    }
}

/// Calculates non-linear fisheye scale factor for an icon at position `icon_x` given pointer at `cursor_x`.
/// Uses a continuous, smooth C1 cosine-power curve to prevent abrupt scaling edges.
pub fn compute_fisheye_scale(cursor_x: f64, icon_x: f64, config: &FisheyeConfig) -> f64 {
    let distance = (cursor_x - icon_x).abs();
    if distance >= config.radius {
        1.0
    } else {
        let norm_dist = distance / config.radius;
        let curve = ((1.0 + (std::f64::consts::PI * norm_dist).cos()) / 2.0).powf(config.curve_exponent);
        1.0 + (config.max_scale - 1.0) * curve
    }
}

/// Computes fisheye scales for a slice of icon center positions.
pub fn compute_fisheye_scales(
    cursor_x: Option<f64>,
    icon_centers: &[f64],
    config: &FisheyeConfig,
) -> Vec<f64> {
    match cursor_x {
        Some(cx) => icon_centers
            .iter()
            .map(|&ix| compute_fisheye_scale(cx, ix, config))
            .collect(),
        None => vec![1.0; icon_centers.len()],
    }
}

/// Second-order mass-spring-damper system for smooth elastic icon scaling animations.
#[derive(Debug, Clone, PartialEq)]
pub struct IconSpring {
    pub current_scale: f64,
    pub velocity: f64,
    pub target_scale: f64,
    pub stiffness: f64,
    pub damping: f64,
    pub mass: f64,
}

impl IconSpring {
    pub fn new(initial_scale: f64) -> Self {
        Self {
            current_scale: initial_scale,
            velocity: 0.0,
            target_scale: initial_scale,
            stiffness: 320.0,
            damping: 22.0,
            mass: 1.0,
        }
    }

    pub fn with_params(initial_scale: f64, stiffness: f64, damping: f64, mass: f64) -> Self {
        Self {
            current_scale: initial_scale,
            velocity: 0.0,
            target_scale: initial_scale,
            stiffness,
            damping,
            mass,
        }
    }

    /// Step spring physics by `dt` seconds using sub-sampled semi-implicit Euler integration.
    pub fn update(&mut self, dt: f64) -> f64 {
        if (self.current_scale - self.target_scale).abs() < 1e-4 && self.velocity.abs() < 1e-4 {
            self.current_scale = self.target_scale;
            self.velocity = 0.0;
            return self.current_scale;
        }

        let max_sub_dt = 0.004;
        let steps = ((dt / max_sub_dt).ceil() as usize).max(1);
        let sub_dt = dt / (steps as f64);

        for _ in 0..steps {
            let displacement = self.current_scale - self.target_scale;
            let spring_force = -self.stiffness * displacement;
            let damping_force = -self.damping * self.velocity;
            let accel = (spring_force + damping_force) / self.mass;

            self.velocity += accel * sub_dt;
            self.current_scale += self.velocity * sub_dt;
        }

        if self.current_scale < 0.1 {
            self.current_scale = 0.1;
            self.velocity = 0.0;
        }

        self.current_scale
    }

    pub fn is_animating(&self) -> bool {
        (self.current_scale - self.target_scale).abs() >= 1e-4 || self.velocity.abs() >= 1e-4
    }
}

/// Dock Engine managing mode, fisheye magnification, and icon spring physics states.
#[derive(Debug, Clone)]
pub struct DockEngine {
    pub mode: DockMode,
    pub fisheye_config: FisheyeConfig,
    pub springs: Vec<IconSpring>,
    pub cursor_x: Option<f64>,
}

impl DockEngine {
    pub fn new(mode: DockMode) -> Self {
        Self {
            mode,
            fisheye_config: FisheyeConfig::default(),
            springs: Vec::new(),
            cursor_x: None,
        }
    }

    pub fn with_fisheye_config(mode: DockMode, config: FisheyeConfig) -> Self {
        Self {
            mode,
            fisheye_config: config,
            springs: Vec::new(),
            cursor_x: None,
        }
    }

    pub fn set_mode(&mut self, mode: DockMode) {
        self.mode = mode;
        if mode.is_efficient() {
            for spring in &mut self.springs {
                spring.target_scale = 1.0;
            }
        }
    }

    pub fn toggle_mode(&mut self) -> DockMode {
        let new_mode = self.mode.toggle();
        self.set_mode(new_mode);
        new_mode
    }

    pub fn sync_icon_count(&mut self, count: usize) {
        if self.springs.len() < count {
            while self.springs.len() < count {
                self.springs.push(IconSpring::new(1.0));
            }
        } else if self.springs.len() > count {
            self.springs.truncate(count);
        }
    }

    pub fn update_cursor(&mut self, cursor_x: Option<f64>, icon_centers: &[f64]) {
        self.sync_icon_count(icon_centers.len());
        self.cursor_x = cursor_x;

        if self.mode.is_fashion() && cursor_x.is_some() {
            let target_scales = compute_fisheye_scales(cursor_x, icon_centers, &self.fisheye_config);
            for (spring, target) in self.springs.iter_mut().zip(target_scales) {
                spring.target_scale = target;
            }
        } else {
            for spring in &mut self.springs {
                spring.target_scale = 1.0;
            }
        }
    }

    pub fn step_physics(&mut self, dt: f64) -> Vec<f64> {
        self.springs.iter_mut().map(|s| s.update(dt)).collect()
    }

    pub fn current_scales(&self) -> Vec<f64> {
        self.springs.iter().map(|s| s.current_scale).collect()
    }

    pub fn is_animating(&self) -> bool {
        self.springs.iter().any(|s| s.is_animating())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fisheye_peak_and_decay() {
        let config = FisheyeConfig::default();
        let cursor_x = 100.0;
        
        let peak_scale = compute_fisheye_scale(cursor_x, 100.0, &config);
        assert!((peak_scale - config.max_scale).abs() < 1e-5);

        let far_scale = compute_fisheye_scale(cursor_x, 100.0 + config.radius + 10.0, &config);
        assert_eq!(far_scale, 1.0);

        let half_scale = compute_fisheye_scale(cursor_x, 100.0 + config.radius / 2.0, &config);
        assert!(half_scale > 1.0 && half_scale < config.max_scale);
    }

    #[test]
    fn test_spring_physics_convergence() {
        let mut spring = IconSpring::new(1.0);
        spring.target_scale = 1.5;

        let dt = 0.016;
        for _ in 0..100 {
            spring.update(dt);
        }

        assert!((spring.current_scale - 1.5).abs() < 0.01);
        assert!(!spring.is_animating());
    }

    #[test]
    fn test_dock_engine_mode_switch() {
        let mut engine = DockEngine::new(DockMode::Fashion);
        assert!(engine.mode.is_fashion());

        let icon_centers = vec![50.0, 100.0, 150.0];
        engine.update_cursor(Some(100.0), &icon_centers);

        assert!(engine.springs[1].target_scale > 1.0);

        engine.toggle_mode();
        assert!(engine.mode.is_efficient());

        engine.update_cursor(Some(100.0), &icon_centers);
        assert_eq!(engine.springs[1].target_scale, 1.0);
    }
}
