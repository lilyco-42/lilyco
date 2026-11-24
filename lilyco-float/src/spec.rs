//! 悬浮层外观与行为参数（纯数据，可序列化，便于从配置文件加载）。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FloatSpec {
    /// 轨道数（同时能容纳几行）
    pub lanes: usize,
    /// 字号（px）
    pub font_size: f64,
    /// 每帧左移像素（60fps 下约等于 speed*60 px/s）
    pub speed: f64,
    /// 整体不透明度 0.3–1
    pub opacity: f64,
    /// 文本颜色（r,g,b 0–1）
    pub color: (f64, f64, f64),
    /// 描边宽度（px），0 表示不描边
    pub stroke: f64,
    /// 初始窗口宽度
    pub width: i32,
    /// 轨道高度（px）
    pub lane_height: i32,
    /// 同时存在的最大条数
    pub max_items: usize,
}

impl Default for FloatSpec {
    fn default() -> Self {
        Self {
            lanes: 4,
            font_size: 22.0,
            speed: 3.2,
            opacity: 0.98,
            color: (1.0, 1.0, 1.0),
            stroke: 3.0,
            width: 960,
            lane_height: 36,
            max_items: 24,
        }
    }
}

impl FloatSpec {
    pub fn height(&self) -> i32 {
        self.lane_height * self.lanes as i32
    }

    /// 把越界参数拉回安全区间，避免配置错误导致窗口不可见。
    pub fn normalized(mut self) -> Self {
        self.lanes = self.lanes.clamp(1, 12);
        self.font_size = self.font_size.clamp(10.0, 96.0);
        self.speed = self.speed.clamp(0.2, 30.0);
        self.opacity = self.opacity.clamp(0.3, 1.0);
        self.stroke = self.stroke.clamp(0.0, 8.0);
        self.width = self.width.clamp(200, 10000);
        self.lane_height = self.lane_height.clamp(16, 160);
        self.max_items = self.max_items.clamp(1, 200);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        let spec = FloatSpec::default();
        let normalized = spec.clone().normalized();
        assert_eq!(normalized.lanes, spec.lanes);
        assert!(normalized.height() > 0);
    }

    #[test]
    fn clamps_bad_values() {
        let spec = FloatSpec {
            lanes: 0,
            font_size: 1000.0,
            speed: -1.0,
            opacity: 0.0,
            ..Default::default()
        }
        .normalized();
        assert_eq!(spec.lanes, 1);
        assert_eq!(spec.font_size, 96.0);
        assert_eq!(spec.speed, 0.2);
        assert_eq!(spec.opacity, 0.3);
    }
}
