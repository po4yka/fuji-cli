package fml

import "list"

generations: [string]: #Generation

#Generation: #DefinitionBase & {
	spec: #GenerationSpec

	#GenerationSpec: {
		name:          string
		capabilities?: #CapabilitySet
	}
}

#CapabilitySet: {
	option_overrides?: [...#OptionCapability]
	raw_conversion?: {
		id: string
		evidence: {
			status: "unverified" | "observed" | "read_verified" | "write_verified"
			manifests: [...string]
		}
		binding: {
			usb_modes: [uint32, ...uint32]
			camera_state?: "still" | "movie"
		}
		read: #RawConversionLayout
		write?: #RawConversionLayout
	}

	#RawConversionLayout: {
		profile_code:         string
		header_padding:       uint32
		declared_field_count: uint16
		total_length:         uint32
		fields: [string, ...string]

		_validation: fields: list.UniqueItems & fields
	}

	#OptionCapability: {
		ref: #RefOption

		_option: options[ref] & {spec: kind: "enum"}

		allowed_values?: [...or([for variant in _option.spec.rules.variants {variant.id}])]
		wire_values?: {
			[Value=or([for variant in _option.spec.rules.variants {variant.id}])]: int | [int, ...int]
		}

		_validation: {
			if allowed_values != _|_ {
				allowed_values: list.UniqueItems & allowed_values
			}
		}
	}

	_validation: {
		if option_overrides != _|_ {
			refs: list.UniqueItems & [for override in option_overrides {override.ref}]
		}
	}
}

// Ref

#RefGeneration: or([for _, generation in generations {generation.id}])

// Definitions

generations: {
	[Name=string]: id: Name

	// TODO: Investigate generation capabilities
	x_trans: #Generation & {
		spec: name: "X-Trans"
	}

	// TODO: Investigate generation capabilities
	x_trans_ii: #Generation & {
		spec: name: "X-Trans II"
	}

	// TODO: Investigate generation capabilities
	x_trans_iii: #Generation & {
		spec: name: "X-Trans III"
	}

	x_trans_iv: #Generation & {
		spec: {
			name: "X-Trans IV"

			_simulation: {
				settings: [
					{ref: "custom_setting_name"},
					{ref: "image_size"},
					{ref: "image_quality"},
					{ref: "film_simulation"},
					{ref: "monochromatic_color_temperature"},
					{ref: "monochromatic_color_tint"},
					{ref: "grain_effect"},
					{ref: "color_chrome_effect"},
					{ref: "color_chrome_fx_blue"},
					{ref: "white_balance"},
					{ref: "white_balance_shift_red"},
					{ref: "white_balance_shift_blue"},
					{ref: "white_balance_temperature"},
					{ref: "highlight_tone"},
					{ref: "shadow_tone"},
					{ref: "color"},
					{ref: "sharpness"},
					{ref: "noise_reduction"},
					{ref: "clarity"},
					{ref: "lens_modulation_optimizer"},
					{ref: "color_space"},
					{ref: "dynamic_range"},
					{ref: "dynamic_range_priority"},
				]

				// TODO: Limit generation option values
				rules: [
					{
						message: "Monochromatic color settings only apply to black and white simulations."
						when: all: [
							{any: [
								{ref: "monochromatic_color_temperature", present: true},
								{ref: "monochromatic_color_tint", present: true},
							]},
							{not: {
								ref: "film_simulation"
								in: ["monochrome", "monochrome_ye", "monochrome_r", "monochrome_g", "acros", "acros_ye", "acros_r", "acros_g"]
							}},
						]
					},
					{
						message: "White balance temperature is only meaningful in Temperature mode."
						when: all: [
							{ref: "white_balance_temperature", present: true},
							{not: {ref: "white_balance", equals: "temperature"}},
						]
					},
					{
						message: "Dynamic Range can only be set when Dynamic Range Priority is disabled."
						when: all: [
							{ref: "dynamic_range", present: true},
							{not: {ref: "dynamic_range_priority", equals: "off"}},
						]
					},
				]
			}

			// TODO: Investigate rendering capabilities
		}
	}

	x_trans_v: #Generation & {
		spec: {
			name: "X-Trans V"
			capabilities: option_overrides: [{
				ref: "film_simulation"
				allowed_values: [for variant in options.film_simulation.spec.rules.variants if variant.id != "reala_ace" {
					variant.id
				}]
				wire_values: options.film_simulation.spec.encoding.spec.values
			}]

			_simulation: {
				settings: list.Concat([x_trans_iv.spec._simulation.settings, [
					{ref: "smooth_skin_effect"},
				]])

				rules: list.Concat([x_trans_iv.spec._simulation.rules, []])
			}

			// TODO: Investigate rendering capabilities
		}
	}
}
