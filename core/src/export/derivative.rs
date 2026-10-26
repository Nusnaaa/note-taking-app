//! Pure, cancellable derivative construction from an authorized snapshot.
use super::*;
#[derive(Default)]
struct Assets {
    bytes: usize,
    values: BTreeMap<String, Vec<u8>>,
}
impl Assets {
    fn insert(&mut self, name: String, value: Vec<u8>) -> Result<()> {
        let bytes = self.bytes - self.values.get(&name).map_or(0, Vec::len);
        if value.len() > snapshot::BYTES.saturating_sub(bytes) {
            return Err(Error::ResourceLimit);
        }
        self.bytes = bytes + value.len();
        self.values.insert(name, value);
        Ok(())
    }
}
impl DerivativeSource {
    pub(crate) fn render(self, control: &JobControl) -> Result<DerivativePlan> {
        let Self {
            library,
            generation,
            scopes,
            records,
            crop,
            mut warnings,
            selected_blocks,
            include_original_ink,
        } = self;
        let mut markdown = String::new();

        let mut assets = Assets::default();

        let mut included = false;

        let mut render_contents = Vec::new();
        for record in &records {
            control.check()?;
            let b = &record.block;
            // PDF and text/assets must use the same reviewed visibility set.
            render_contents.push(b.content.clone());
            if !b.has_table_ink() {
                render_contents.extend(b.regions.iter().filter_map(|r| r.content.clone()));
            }
            for region in b.regions.iter().filter(|_| !b.has_table_ink()) {
                if let Some(content) = &region.content {
                    let converted = TextConverter.convert(content, Format::Markdown, control)?;
                    markdown += &converted.text;
                    markdown += "\n\n";
                    warnings.extend(converted.fidelity.unsupported);
                }
            }
            match &b.content {
                Content::Ink => {
                    warnings.push("Unconverted handwriting omitted from converted text.".into())
                }

                Content::Table(t) => {
                    if t.layout.is_some() && include_original_ink {
                        let name = format!("table-ink-{}.svg", b.id);
                        assets.insert(
                            name.clone(),
                            table_ink::table_ink_svg(b, &record.ink, control)?.into_bytes(),
                        )?;
                        markdown += &format!("![Table ink and grid in current geometry]({name})\n");
                        warnings.push("Table ink/grid SVG uses current cell placement and block transform; editable cell values remain in the text derivative. It does not render a saved view, embeddings or physical screen units.".into());
                        included |= !b.visible_strokes.is_empty();
                    }
                    if t.layout.is_some() {
                        warnings.push("Table cell dimensions are omitted from the text derivative; explicit ink export also provides a current ink/grid SVG and separate original-coordinate assets.".into());
                    }
                    if b.regions.iter().any(|region| region.content.is_none()) {
                        warnings.push("Unconverted table-cell handwriting omitted from converted text; existing editable cell values are retained.".into());
                    }

                    let text = t
                        .rows
                        .iter()
                        .map(|row| {
                            row.iter()
                                .map(|c| {
                                    c.value.literal().replace('|', "\\|").replace('\n', "<br>")
                                })
                                .collect::<Vec<_>>()
                                .join(" | ")
                        })
                        .collect::<Vec<_>>();

                    markdown += &format!(
                        "| {} |\n| {} |\n",
                        text[0],
                        vec!["---"; t.rows[0].len()].join(" | ")
                    );

                    for row in &text[1..] {
                        markdown += &format!("| {row} |\n");
                    }

                    warnings.push("Markdown table presentation uses the first row as a header; cell maths structure remains native-only.".into());
                }

                Content::Diagram(_) | Content::Graph(_) => {
                    let bounds =
                        crop.or(record.layout.as_ref().map(|l| l.bounds))
                            .unwrap_or(Rect {
                                x: 0.,
                                y: 0.,
                                width: 100.,
                                height: 100.,
                            });

                    if crop.is_some() {
                        if let Content::Diagram(diagram) = &b.content {
                            for part in &diagram.parts {
                                let contained = match &part.shape {
                                    Shape::Polyline(points) => {
                                        points.iter().all(|p| bounds.contains(*p))
                                    }

                                    Shape::Rectangle(r) | Shape::Ellipse(r) => {
                                        bounds.contains(crate::objects::Position { x: r.x, y: r.y })
                                            && bounds.contains(crate::objects::Position {
                                                x: r.x + r.width,
                                                y: r.y + r.height,
                                            })
                                    }
                                };

                                if !contained {
                                    return Err(Error::ReviewRequired);
                                }
                            }
                        }
                    }

                    let name = format!("object-{}.svg", b.id);

                    assets.insert(
                        name.clone(),
                        technical_svg(&b.content, bounds)?.into_bytes(),
                    )?;

                    markdown += &format!("![Technical object]({name})\n");

                    warnings.push("Vector derivative retains drawn data; component editability requires native format.".into());
                }

                Content::Image(i) => {
                    let (mime, bytes) = record
                        .image
                        .as_ref()
                        .ok_or(Error::Integrity)?
                        .as_ref()
                        .clone();
                    let (name, output) = if let Some(crop) = i.crop {
                        let (cropped, rounded) =
                            image_crop::crop_image(&mime, &bytes, crop, control)?;
                        warnings.push("Cropped image exported as 8-bit RGBA PNG without original metadata; original bytes remain native-only.".into());
                        if rounded {
                            warnings.push(
                                "Image crop rounded inward to complete source pixels.".into(),
                            );
                        }
                        (format!("image-{}-crop-{}.png", i.asset, b.id), cropped)
                    } else {
                        (
                            format!(
                                "image-{}.{}",
                                i.asset,
                                if mime == "image/png" { "png" } else { "jpg" }
                            ),
                            bytes,
                        )
                    };
                    assets.insert(name.clone(), output)?;
                    markdown +=
                        &format!("![{}]({name})\n", i.caption.replace(['[', ']', '\n'], " "));
                }

                c => {
                    let converted = TextConverter.convert(c, Format::Markdown, control)?;

                    markdown += &converted.text;

                    warnings.extend(converted.fidelity.unsupported);
                }
            }

            if include_original_ink && !b.source_ids.is_empty() {
                let source = &record.ink;

                let mut body = String::new();

                let mut min_x = f64::INFINITY;

                let mut min_y = f64::INFINITY;

                let mut max_x = f64::NEG_INFINITY;

                let mut max_y = f64::NEG_INFINITY;

                for s in source {
                    control.check()?;
                    if !b.visible_strokes.contains(&s.id) {
                        continue;
                    }

                    let (colour, width) = if let Some(style) = s.style {
                        (style.colour_rgba, style.width)
                    } else {
                        warnings.push("Legacy ink appearance was not recorded; original-ink SVG uses a black, unit-width fallback.".into());
                        ([0, 0, 0, 255], 1.)
                    };
                    if let Some((derived, bounds)) = source_ink::pressure_svg_budget(
                        s,
                        (8usize * 1024 * 1024).saturating_sub(body.len()),
                    )? {
                        if s.points.iter().any(|p| p.pressure.is_none()) {
                            warnings.push("Pressure was unavailable for some source samples; their recorded base width is used.".into());
                        }
                        if s.points
                            .iter()
                            .any(|p| p.tilt_rad.is_some() || p.orientation_rad.is_some())
                        {
                            warnings.push(
                                "Tilt/orientation-dependent nib appearance is not reconstructed."
                                    .into(),
                            );
                        }
                        if s.tool == crate::ink::Tool::Eraser {
                            warnings.push("Eraser observations are shown as source paths, not replayed as destructive edits.".into());
                        }
                        min_x = min_x.min(bounds.x);
                        min_y = min_y.min(bounds.y);
                        max_x = max_x.max(bounds.x + bounds.width);
                        max_y = max_y.max(bounds.y + bounds.height);
                        body += &derived;
                        if body.len() > 8 * 1024 * 1024 {
                            return Err(Error::ResourceLimit);
                        }
                        continue;
                    }
                    if s.points.iter().any(|p| {
                        p.pressure.is_some() || p.tilt_rad.is_some() || p.orientation_rad.is_some()
                    }) {
                        warnings.push("Original-ink SVG preserves recorded base colour/width; pressure/tilt-dependent appearance is not reconstructed.".into());
                    }
                    if s.tool == crate::ink::Tool::Eraser {
                        warnings.push("Eraser observations are shown as source paths, not replayed as destructive edits.".into());
                    }
                    let margin = width / 2.;
                    for p in &s.points {
                        min_x = min_x.min(p.x - margin);
                        min_y = min_y.min(p.y - margin);
                        max_x = max_x.max(p.x + margin);
                        max_y = max_y.max(p.y + margin);
                    }
                    let rgb = format!("#{:02x}{:02x}{:02x}", colour[0], colour[1], colour[2]);
                    let opacity = colour[3] as f64 / 255.;
                    if s.points.len() == 1 {
                        let p = &s.points[0];
                        body += &format!("<circle cx=\"{}\" cy=\"{}\" r=\"{margin}\" fill=\"{rgb}\" fill-opacity=\"{opacity}\"/>", p.x, p.y);
                    } else {
                        body += &format!("<polyline fill=\"none\" stroke=\"{rgb}\" stroke-opacity=\"{opacity}\" stroke-width=\"{width}\" stroke-linecap=\"round\" stroke-linejoin=\"round\" points=\"{}\"/>",source_ink::point_list(s, control)?);
                    }
                    if body.len() > 8 * 1024 * 1024 {
                        return Err(Error::ResourceLimit);
                    }
                }

                if body.len() > 8 * 1024 * 1024 {
                    return Err(Error::ResourceLimit);
                }
                if !body.is_empty() {
                    let bounds = Rect {
                        x: min_x - 1.,
                        y: min_y - 1.,
                        width: (max_x - min_x + 2.).max(2.),
                        height: (max_y - min_y + 2.).max(2.),
                    };

                    bounds.validate()?;

                    let name = format!("original-{}.svg", b.id);

                    assets.insert(name.clone(), svg_root(body, bounds).into_bytes())?;

                    markdown += &format!("\n![Original handwriting]({name})\n");

                    included = true;
                }
            }

            markdown += "\n\n";
            if markdown.len() > snapshot::BYTES.saturating_sub(assets.bytes) {
                return Err(Error::ResourceLimit);
            }
        }

        warnings.sort();

        warnings.dedup();

        control.check()?;

        let bundle = ExportBundle {
            markdown,
            assets: assets.values,
            warnings: warnings.clone(),
            includes_original_ink: included,
        };

        Ok(DerivativePlan {
            library,
            generation,
            scopes,
            bundle,
            render_contents,
            selected_blocks,
            warnings,
            includes_original_ink: included,
        })
    }
}

impl DerivativePlan {
    pub(crate) fn stamp(&self) -> crate::portable::ExportStamp {
        crate::portable::ExportStamp::new(
            self.library.clone(),
            self.generation.clone(),
            self.scopes.clone(),
        )
    }
    pub(crate) fn into_bundle(self) -> ExportBundle {
        self.bundle
    }
    pub(crate) fn render_pdf(
        self,
        font: &[u8],
        page: Page,
        control: Option<&JobControl>,
    ) -> Result<crate::render::PdfOutput> {
        if self.includes_original_ink {
            return Err(Error::Unsupported);
        }
        let mut output =
            crate::render::render_pdf_control(&self.render_contents, font, page, control)?;
        output.warnings.extend(self.warnings);
        output.warnings.push("PDF is a typeset content derivative; saved canvas/view geometry and original ink are not rendered.".into());
        output.warnings.sort();
        output.warnings.dedup();
        Ok(output)
    }
}
