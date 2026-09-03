package fml

import "list"

cameras: [string]: #Camera

// PTP datatype code of each option's SimulationSetting wire codec, keyed by
// option id: PTP strings for string options, otherwise the 16-bit scalar whose
// signedness follows the smallest wire value (codegen uses the same rule and
// rejects a static descriptor that disagrees with it).
_ptp_data_type: {
	for id, option in options {
		let _spec = option.spec
		if _spec.kind == "string" {
			"\(id)": 0xFFFF
		}
		if _spec.kind != "string" {
			if _spec.encoding.kind == "lookup" {
				let _min = list.Min(list.FlattenN([for _, v in _spec.encoding.spec.values {v}], 1))
				if _min < 0 {"\(id)": 0x0003}
				if _min >= 0 {"\(id)": 0x0004}
			}
			if _spec.encoding.kind != "lookup" && _spec.rules != _|_ && _spec.rules.min != _|_ {
				if _spec.rules.min < 0 {"\(id)": 0x0003}
				if _spec.rules.min >= 0 {"\(id)": 0x0004}
			}
		}
	}
}

#Camera: #DefinitionBase & {
	spec: #Spec

	#Spec: {
		name:       string
		generation: #RefGeneration

		_generation: generations["\(generation)"].spec

		usb:  #USB
		ptp?: #PTPIdentity
		preflight?: [...#PreflightProfile]
		capabilities?: #CameraCapabilities

		features?: #Features

		#USB: {
			#FujifilmVendorID:                 0x04cb
			#DefaultCameraUSBChunkSizeCeiling: 1024 * 1024

			vendor_id:          uint16 | *#FujifilmVendorID
			product_id:         uint16
			// The runtime's bulk read window is capped at 16 MiB
			// (MAX_PTP_BULK_READ_CHUNK_BYTES in src/lib/ptp/mod.rs); a larger
			// ceiling could never be reached and would only retry promotion.
			chunk_size_ceiling: (uint & <=16777216) | *#DefaultCameraUSBChunkSizeCeiling
		}

		#PTPIdentity: {
			manufacturer: string & =~".+"
			model:        string & =~".+"
		}

		#CameraCapabilities: {
			generation: #CapabilitySet
			model:      #CapabilitySet
			firmware: [Firmware=string & =~"^[0-9]+\\.[0-9]+$"]: #CapabilitySet
		}

		#PreflightProfile: {
			operation: "backup_restore" | "simulation_access" | "simulation_write" | "raw_conversion" | "raw_recovery_fetch" | "raw_recovery_cleanup"
			status:    "verified" | "unverified"
			firmware:  string & =~".+"

			minimum_battery_percent: uint8 & <=100
			allowed_usb_modes: [...uint32]
			required_operations: [uint16, ...uint16]
			required_properties: [#RequiredProperty, ...#RequiredProperty]

			if status == "verified" {
				allowed_usb_modes: [uint32, ...uint32]
			}

			#RequiredProperty: {
				code:       uint16
				data_type?: uint16
				writable:   bool

				// A descriptor the runtime may substitute when the camera refuses
				// `GetDevicePropDesc` with a PTP response code. It exists only to
				// authorize writes, so it pins the datatype and asserts
				// writability; the live value must still decode exactly and fall
				// inside `form`. `evidence` names where the shape comes from.
				static_descriptor?: #StaticDescriptor

				if static_descriptor != _|_ {
					data_type: uint16
					writable:  true
					if data_type == 0xFFFF {
						static_descriptor: form: kind: "none"
					}
				}

				#StaticDescriptor: {
					evidence: string & =~".+"
					form:     #StaticForm
				}

				#StaticForm: {kind: "none"} |
					{kind: "enumeration", values: [int, ...int]} |
					{kind: "range", minimum: int, maximum: int, step: int & >0}
			}

			_validation: {
				allowed_usb_modes:   list.UniqueItems & allowed_usb_modes
				required_operations: list.UniqueItems & required_operations
				required_properties: list.UniqueItems & [for property in required_properties {property.code}]
			}
		}

		#Features: {
			backup?:     true
			simulation?: #Simulation
			render?:     #Render

			#Simulation: {
				slots: uint
				settings: [...#Setting]

				#Setting: {
					id:  string | *ref
					ref: #RefOption
				}

				#Grammar: #GrammarBase & {
					_ids: {
						all: [for _, setting in settings {setting.id}]
						i: [for _, setting in settings if options[setting.ref].spec.kind == "integer" {setting.id}]
						f: [for _, setting in settings if options[setting.ref].spec.kind == "float" {setting.id}]
						s: [for _, setting in settings if options[setting.ref].spec.kind == "string" {setting.id}]
						e: [for _, setting in settings if options[setting.ref].spec.kind == "enum" {setting.id}]
					}
				}

				transformations?: [...#Transformation]

				#Transformation: #TransformationBase & {
					when?: #Grammar.#Predicate
					apply: [...#Grammar.#Assignment]
				}

				rules?: [...#Rule]

				#Rule: #RuleBase & {
					when: #Grammar.#Predicate
				}

				_validation: {
					ids: list.UniqueItems & [for s in settings {s.id}]
				}
			}

			#Render: {
				profile_code:   uint32
				header_padding: uint32

				fields: [...#Field]

				#Field: {
					id:          string | *ref
					skip_read?:  true
					skip_write?: true
					ref?:        #RefOption
				}

				#Grammar: #GrammarBase & {
					_scoped: true
					_ids: {
						all: [for _, field in fields {field.id}]
						i: list.Concat([
							[for _, field in fields if field.ref == _|_ {field.id}],
							[for _, field in fields if field.ref != _|_ if options[field.ref].spec.kind == "integer" {field.id}],
						])
						f: [for _, field in fields if field.ref != _|_ if options[field.ref].spec.kind == "float" {field.id}]
						s: [for _, field in fields if field.ref != _|_ if options[field.ref].spec.kind == "string" {field.id}]
						e: [for _, field in fields if field.ref != _|_ if options[field.ref].spec.kind == "enum" {field.id}]
					}
				}

				#TransformationGrammar: #GrammarBase & {
					_scoped: false
					_ids:    #Grammar._ids
				}

				transformations?: [...#Transformation]

				#Transformation: #TransformationBase & {
					when?: #TransformationGrammar.#Predicate
					apply: [...#TransformationGrammar.#Assignment]
				}

				rules?: [...#Rule]

				#Rule: #RuleBase & {
					when: #Grammar.#Predicate
				}

				_validation: {
					ids: list.UniqueItems & [for f in fields {f.id}]
				}
			}
		}
	}
}

// Ref

#RefCamera: or([for _, camera in cameras {camera.id}])

// Definitions

cameras: {
	[Name=string]: id: Name

	x_e1: #Camera & {
		spec: {
			name:       "FUJIFILM X-E1"
			generation: "x_trans"
			usb: product_id: 0x0283
		}
	}

	x_m1: #Camera & {
		spec: {
			name:       "FUJIFILM X-M1"
			generation: "x_trans"
			usb: product_id: 0x02b6
		}
	}

	x70: #Camera & {
		spec: {
			name:       "FUJIFILM X70"
			generation: "x_trans_ii"
			usb: product_id: 0x02ba
		}
	}

	x_e2: #Camera & {
		spec: {
			name:       "FUJIFILM X-E2"
			generation: "x_trans_ii"
			usb: product_id: 0x02b5
		}
	}

	x_t1: #Camera & {
		spec: {
			name:       "FUJIFILM X-T1"
			generation: "x_trans_ii"
			usb: product_id: 0x02bf
		}
	}

	x_t10: #Camera & {
		spec: {
			name:       "FUJIFILM X-T10"
			generation: "x_trans_ii"
			usb: product_id: 0x02c8
		}
	}

	x100f: #Camera & {
		spec: {
			name:       "FUJIFILM X100F"
			generation: "x_trans_iii"
			usb: product_id: 0x02d1
		}
	}

	x_e3: #Camera & {
		spec: {
			name:       "FUJIFILM X-E3"
			generation: "x_trans_iii"
			usb: product_id: 0x02d6
		}
	}

	x_h1: #Camera & {
		spec: {
			name:       "FUJIFILM X-H1"
			generation: "x_trans_iii"
			usb: product_id: 0x02d7
		}
	}

	x_pro2: #Camera & {
		spec: {
			name:       "FUJIFILM X-Pro2"
			generation: "x_trans_iii"
			usb: product_id: 0x02cb
		}
	}

	x_t2: #Camera & {
		spec: {
			name:       "FUJIFILM X-T2"
			generation: "x_trans_iii"
			usb: product_id: 0x02cd
		}
	}

	x_t20: #Camera & {
		spec: {
			name:       "FUJIFILM X-T20"
			generation: "x_trans_iii"
			usb: product_id: 0x02d4
		}
	}

	x100v: #Camera & {
		spec: {
			name:       "FUJIFILM X100V"
			generation: "x_trans_iv"
			usb: product_id: 0x02e5
		}
	}

	x_e4: #Camera & {
		spec: {
			name:       "FUJIFILM X-E4"
			generation: "x_trans_iv"
			usb: product_id: 0x02e8
		}
	}

	x_pro3: #Camera & {
		spec: {
			name:       "FUJIFILM X-Pro3"
			generation: "x_trans_iv"
			usb: product_id: 0x02e4
		}
	}

	x_s10: #Camera & {
		spec: {
			name:       "FUJIFILM X-S10"
			generation: "x_trans_iv"
			usb: product_id: 0x02ea
		}
	}

	x_t3: #Camera & {
		spec: {
			name:       "FUJIFILM X-T3"
			generation: "x_trans_iv"
			usb: product_id: 0x02dd
		}
	}

	x_t4: #Camera & {
		spec: {
			name:       "FUJIFILM X-T4"
			generation: "x_trans_iv"
			usb: product_id: 0x02e6
		}
	}

	x100vi: #Camera & {
		spec: {
			name:       "FUJIFILM X100VI"
			generation: "x_trans_v"
			usb: product_id: 0x0305
		}
	}

	x_h2: #Camera & {
		spec: {
			name:       "FUJIFILM X-H2"
			generation: "x_trans_v"
			usb: product_id: 0x02f2
		}
	}

	x_h2s: #Camera & {
		spec: {
			name:       "FUJIFILM X-H2S"
			generation: "x_trans_v"
			usb: product_id: 0x02f0
		}
	}

	x_s20: #Camera & {
		spec: {
			name:        "FUJIFILM X-S20"
			generation:  "x_trans_iv"
			_generation: _

			usb: product_id: 0x02f7

			features: {
				backup: true

				simulation: {
					slots:    4
					settings: _generation._simulation.settings
					rules:    _generation._simulation.rules
				}
			}
		}
	}

	x_t5: #Camera & {
		spec: {
			name:        "FUJIFILM X-T5"
			generation:  "x_trans_v"
			_generation: _

			usb: {
				product_id:         0x02fc
				chunk_size_ceiling: 16128 * 1024
			}
			ptp: {
				manufacturer: "FUJIFILM"
				model:        "X-T5"
			}
			// Every enum consumed by an X-T5 mutating path is pinned here. This
			// prevents the runtime from falling back to a global option encoding.
			_x_t5_enum_capabilities: [
				{
					ref: "custom_setting"
					allowed_values: ["c1", "c2", "c3", "c4", "c5", "c6", "c7"]
					wire_values: {c1: 1, c2: 2, c3: 3, c4: 4, c5: 5, c6: 6, c7: 7}
				},
				{
					ref: "color_chrome_effect"
					allowed_values: ["strong", "weak", "off"]
					wire_values: {strong: 3, weak: 2, off: 1}
				},
				{
					ref: "color_chrome_fx_blue"
					allowed_values: ["strong", "weak", "off"]
					wire_values: {strong: 3, weak: 2, off: 1}
				},
				{
					ref: "color_space"
					allowed_values: ["srgb", "adobe_rgb"]
					wire_values: {srgb: 2, adobe_rgb: 1}
				},
				{
					ref: "dynamic_range"
					allowed_values: ["auto", "hdr100", "hdr200", "hdr400", "hdr800", "hdr800_plus"]
					wire_values: {auto: 65535, hdr100: 100, hdr200: 200, hdr400: 400, hdr800: 800, hdr800_plus: 1600}
				},
				{
					ref: "dynamic_range_priority"
					allowed_values: ["auto", "plus", "strong", "weak", "off"]
					wire_values: {auto: 32768, plus: 3, strong: 2, weak: 1, off: 0}
				},
				{
					ref: "file_type"
					allowed_values: ["jpeg", "heif", "tiff8", "tiff16"]
					wire_values: {jpeg: 7, heif: 18, tiff8: 9, tiff16: 11}
				},
				{
					ref: "film_simulation"
					allowed_values: [
						"provia", "velvia", "astia", "pro_neg_hi", "pro_neg_std",
						"monochrome", "monochrome_ye", "monochrome_r", "monochrome_g", "sepia",
						"classic_chrome", "acros", "acros_ye", "acros_r", "acros_g", "eterna",
						"classic_negative", "eterna_bleach_bypass", "nostalgic_negative",
					]
					wire_values: {
						provia:             1, velvia:          2, astia:             3, pro_neg_hi:            4, pro_neg_std: 5
						monochrome:         6, monochrome_ye:   7, monochrome_r:      8, monochrome_g:          9
						sepia:              10, classic_chrome: 11, acros:            12, acros_ye:             13, acros_r: 14
						acros_g:            15, eterna:         16, classic_negative: 17, eterna_bleach_bypass: 18
						nostalgic_negative: 19, reala_ace:      20
					}
				},
				{
					ref: "grain_effect"
					allowed_values: ["strong_large", "weak_large", "strong_small", "weak_small", "off"]
					wire_values: {strong_large: 5, weak_large: 4, strong_small: 3, weak_small: 2, off: 1}
				},
				{
					ref: "image_quality"
					allowed_values: ["fine_raw", "fine", "normal_raw", "normal", "raw"]
					wire_values: {fine_raw: 4, fine: 2, normal_raw: 5, normal: 3, raw: 1}
				},
				{
					ref: "image_size"
					allowed_values: [
						"7728x5152", "7728x4344", "5152x5152", "6864x5152", "6432x5152",
						"5472x3648", "5472x3080", "3648x3648", "4864x3648", "4560x3648",
						"3888x2592", "3888x2184", "2592x2592", "3456x2592", "3264x2592",
					]
					wire_values: {
						"7728x5152": 7, "7728x4344":  8, "5152x5152":  9, "6864x5152": 14
						"6432x5152": 16, "5472x3648": 4, "5472x3080":  5, "3648x3648": 6
						"4864x3648": 18, "4560x3648": 20, "3888x2592": 1, "3888x2184": 2
						"2592x2592": 3, "3456x2592":  10, "3264x2592": 12
					}
				},
				{
					ref: "lens_modulation_optimizer"
					allowed_values: ["on", "off"]
					wire_values: {on: 1, off: 2}
				},
				{
					ref: "smooth_skin_effect"
					allowed_values: ["strong", "weak", "off"]
					wire_values: {strong: 3, weak: 2, off: 1}
				},
				{
					ref: "teleconverter"
					allowed_values: ["on", "off"]
					wire_values: {on: 1, off: 2}
				},
				{
					ref: "white_balance"
					allowed_values: [
						"as_shot", "white_priority", "auto", "ambience_priority", "custom1", "custom2",
						"custom3", "temperature", "daylight", "shade", "fluorescent1", "fluorescent2",
						"fluorescent3", "incandescent", "underwater",
					]
					wire_values: {
						as_shot:      0, white_priority:   32800, auto:         2, ambience_priority: 32801
						custom1:      32776, custom2:      32777, custom3:      32778, temperature:   32775
						daylight:     4, shade:            32774, fluorescent1: 32769, fluorescent2:  32770
						fluorescent3: 32771, incandescent: 6, underwater:       8
					}
				},
			]
			_x_t5_post_reala_film_simulations: [
				"provia", "velvia", "astia", "pro_neg_hi", "pro_neg_std",
				"monochrome", "monochrome_ye", "monochrome_r", "monochrome_g", "sepia",
				"classic_chrome", "acros", "acros_ye", "acros_r", "acros_g", "eterna",
				"classic_negative", "eterna_bleach_bypass", "nostalgic_negative", "reala_ace",
			]
			capabilities: {
				generation: _generation.capabilities
				model: option_overrides: _x_t5_enum_capabilities
				firmware: {
					"3.01": {}
					"4.00": option_overrides: [{
						ref:            "film_simulation"
						allowed_values: _x_t5_post_reala_film_simulations
					}]
					"4.31": {
						option_overrides: [{
							ref:            "film_simulation"
							allowed_values: _x_t5_post_reala_film_simulations
						}]
						raw_conversion: {
							id: "x_t5-4.31-raw-layout-unverified"
							// Evidence state (2026-09-03): the 8-character profile
							// code plus 0x1ee of padding places the first value at
							// wire offset 0x201, which agrees with libfuji's captured
							// D185 construction and with the verified 629-byte write
							// length. libfuji's X-T5 captures embed the code
							// "FF129504" (firmware unknown) and the live D184 pair on
							// 4.31 reads "F179502,FA179502", so the code value itself
							// is unconfirmed until a D185 capture with a RAF loaded.
							// Codegen cross-checks this layout against the render
							// feature above; it cannot check it against the camera.
							evidence: {
								status: "unverified"
								manifests: []
							}
							binding: usb_modes: [0x6]
							read: {
								profile_code:         "ff179502"
								header_padding:       0x1ee
								declared_field_count: 29
								total_length:         625
								fields: [for field in features.render.fields if field.skip_read == _|_ {field.id}]
							}
							write: {
								profile_code:         "ff179502"
								header_padding:       0x1ee
								declared_field_count: 29
								total_length:         629
								fields: [for field in features.render.fields if field.skip_write == _|_ {field.id}]
							}
						}
					}
				}
			}

			_preflight_common_properties: [
				{code: 0xD16E, data_type: 0x0004, writable: false},
				{code: 0xD36B, data_type: 0xFFFF, writable: false},
			]
			// The X-T5 on 4.31 refuses GetDevicePropDesc for every property in
			// USB mode 0x6 (x-t5-device-audit-2026-08-31), so the writable
			// selector and settings carry static descriptors. The selector shape
			// comes from the firmware image's own descriptor table; the settings
			// have no static row there, so their datatype is derived from the
			// option encoding and their writability is asserted, not yet
			// device-verified (x-t5-firmware-4.31-static-analysis-2026-09-03).
			_preflight_simulation_selector: {
				code:      0xD18C
				data_type: 0x0004
				writable:  true
				static_descriptor: {
					evidence: "FWUP0030.DAT 4.31 descriptor table: UINT16 get/set enumeration, default 0x0001; slot count from features.simulation.slots and D1A5 = 7 in the 2026-08-31 device audit"
					form: {kind: "enumeration", values: list.Range(1, features.simulation.slots+1, 1)}
				}
			}
			_preflight_simulation_setting_descriptors: [for setting in features.simulation.settings {
				code:      options[setting.ref].spec.encoding.prop_code
				data_type: _ptp_data_type[setting.ref]
				writable:  true
				static_descriptor: {
					evidence: "2026-08-31 device audit: value decodes against the option wire type; FWUP0030.DAT 4.31 lists the code in its tether property table without a static descriptor row; writability not yet device-verified"
					form: kind: "none"
				}
			}]
			_preflight_simulation_properties: list.Concat([
				_preflight_common_properties,
				[_preflight_simulation_selector],
				_preflight_simulation_setting_descriptors,
			])
			_preflight_simulation_access_properties: list.Concat([
				_preflight_common_properties,
				[_preflight_simulation_selector],
				[for setting in features.simulation.settings {
					{code: options[setting.ref].spec.encoding.prop_code, writable: false}
				}],
			])
			_preflight_raw_conversion_properties: list.Concat([
				_preflight_common_properties,
				[
					{code: 0xD183, data_type: 0x0004, writable: true},
					{code: 0xD185, writable: true},
				],
			])

			preflight: [
				{
					operation:               "backup_restore"
					status:                  "verified"
					firmware:                "4.31"
					minimum_battery_percent: 100
					allowed_usb_modes: [0x6]
					required_operations: [0x1001, 0x1008, 0x1009, 0x100C, 0x100D, 0x1014, 0x1015]
					required_properties: _preflight_common_properties
				},
				{
					operation:               "simulation_access"
					status:                  "unverified"
					firmware:                "4.31"
					minimum_battery_percent: 100
					allowed_usb_modes: [0x6]
					required_operations: [0x1001, 0x1014, 0x1015, 0x1016]
					required_properties: _preflight_simulation_access_properties
				},
				{
					operation:               "simulation_write"
					status:                  "unverified"
					firmware:                "4.31"
					minimum_battery_percent: 100
					allowed_usb_modes: [0x6]
					required_operations: [0x1001, 0x1014, 0x1015, 0x1016]
					required_properties: _preflight_simulation_properties
				},
				{
					operation:               "raw_conversion"
					status:                  "unverified"
					firmware:                "4.31"
					minimum_battery_percent: 100
					allowed_usb_modes: [0x6]
					required_operations: [0x1001, 0x1007, 0x1008, 0x1009, 0x100B, 0x1014, 0x1015, 0x1016, 0x900C, 0x900D]
					required_properties: _preflight_raw_conversion_properties
				},
				{
					operation:               "raw_recovery_fetch"
					status:                  "verified"
					firmware:                "4.31"
					minimum_battery_percent: 0
					allowed_usb_modes: [0x6]
					required_operations: [0x1001, 0x1008, 0x1009, 0x1014, 0x1015]
					required_properties: _preflight_common_properties
				},
				{
					operation:               "raw_recovery_cleanup"
					status:                  "verified"
					firmware:                "4.31"
					minimum_battery_percent: 100
					allowed_usb_modes: [0x6]
					// Cleanup re-reads GetObjectInfo (0x1008) before DeleteObject and
					// proves the deletion through GetObjectHandles (0x1007); both are
					// advertised in USB mode 0x6 (x-t5-device-audit-2026-08-31).
					required_operations: [0x1001, 0x1007, 0x1008, 0x100B, 0x1014, 0x1015]
					required_properties: _preflight_common_properties
				},
			]

			features: {
				backup: true

				simulation: {
					slots:    7
					settings: _generation._simulation.settings
					rules:    _generation._simulation.rules
				}

				// TODO: Extract common info to generation
				render: {
					profile_code:   0xff179502
					header_padding: 0x1ee
					fields: [
						{id: "head_0"},
						{ref: "file_type"},
						{ref: "image_size"},
						{ref: "image_quality"},
						{ref: "exposure_offset"},
						{ref: "dynamic_range"},
						{ref: "dynamic_range_priority"},
						{ref: "film_simulation"},
						{ref: "grain_effect"},
						{ref: "color_chrome_effect"},
						{id: "white_balance_as_shot"},
						{ref: "white_balance"},
						{ref: "white_balance_shift_red"},
						{ref: "white_balance_shift_blue"},
						{ref: "white_balance_temperature"},
						{ref: "highlight_tone"},
						{ref: "shadow_tone"},
						{ref: "color"},
						{ref: "sharpness"},
						{ref: "noise_reduction"},
						{ref: "lens_modulation_optimizer"},
						{ref: "color_space"},
						{ref: "monochromatic_color_temperature"},
						{ref: "smooth_skin_effect"},
						{ref: "color_chrome_fx_blue"},
						{ref: "monochromatic_color_tint"},
						{ref: "clarity"},
						{ref: "teleconverter"},
						{
							id:        "tail_0"
							skip_read: true
						},
					]
					transformations: [
						{
							when: {ref: "dynamic_range", equals: "hdr800_plus"}
							apply: [
								{ref: "dynamic_range", value: "hdr800"},
								{ref: "dynamic_range_priority", value: "plus"},
							]
						},
						{
							when: {ref: "white_balance", equals: "as_shot"}
							apply: [{ref: "white_balance_as_shot", value: 0x01}]
						},
						{
							when: {not: {ref: "white_balance", equals: "as_shot"}}
							apply: [{ref: "white_balance_as_shot", value: 0x02}]
							one_way: true
						},
						{
							apply: [
								{ref: "head_0", value: 0},
								{ref: "tail_0", value: 0},
							]
							one_way: true
						},
					]
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
							message: "White balance shifts apply only when White Balance is not As Shot."
							when: all: [
								{ref: "white_balance", equals: "as_shot"},
								{any: [
									{ref: "white_balance_shift_red", present: true},
									{ref: "white_balance_shift_blue", present: true},
									{ref: "white_balance_temperature", present: true},
								]},
							]
						},
						{
							message: "Dynamic Range cannot exceed the value the image was shot with."
							when: any: [
								{all: [
									{ref: "dynamic_range", scope: "original", equals: "hdr100"},
									{not: {ref: "dynamic_range", in: ["hdr100"]}},
								]},
								{all: [
									{ref: "dynamic_range", scope: "original", equals: "hdr200"},
									{not: {ref: "dynamic_range", in: ["hdr100", "hdr200"]}},
								]},
								{all: [
									{ref: "dynamic_range", scope: "original", equals: "hdr400"},
									{not: {ref: "dynamic_range", in: ["hdr100", "hdr200", "hdr400"]}},
								]},
								{all: [
									{ref: "dynamic_range", scope: "original", equals: "hdr800"},
									{not: {ref: "dynamic_range", in: ["hdr100", "hdr200", "hdr400", "hdr800"]}},
								]},
							]
						},
					]
				}
			}
		}
	}
}
