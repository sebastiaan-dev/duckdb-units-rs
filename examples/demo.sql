.bail on
.mode duckbox

.print ''
.print '1. Normalize mixed energy readings into kilowatt-hours (UCUM: kW.h)'
WITH energy_readings(site, value, source_unit) AS (
    VALUES ('Plant A', 3600000.0, 'J'),
           ('Plant B', 1250.0, 'W.h'),
           ('Plant C', 2.5, 'kW.h'),
           ('Plant D', 7.2, 'MJ')
)
SELECT site, value, source_unit,
       round(unit_convert(value, source_unit, 'kW.h'), 6) AS kwh
FROM energy_readings;

.print ''
.print '2. Compound expressions and dimensional compatibility'
SELECT unit_convert(1000, 'kg/m3', 'g/L') AS density_g_per_litre,
       unit_compatible('Pa', 'kg/(m.s2)') AS pressure_matches,
       unit_compatible('kW.h', 'kg') AS energy_matches_mass;

.print ''
.print '3. Absolute temperatures are NOT temperature differences'
SELECT unit_convert(20, 'Cel', '[degF]') AS absolute_fahrenheit,
       unit_convert_delta(20, 'Cel', '[degF]') AS difference_fahrenheit,
       unit_convert(20, 'Cel', 'K') AS absolute_kelvin,
       unit_convert_delta(20, 'Cel', 'K') AS difference_kelvin;

.print ''
.print '4. Audit dirty data without aborting the scan'
WITH readings(id, value, source_unit) AS (
    VALUES (1, 3600000.0, 'J'),
           (2, 1.0, 'kg'),
           (3, 1.0, 'watts'),
           (4, NULL, 'J'),
           (5, 1.0, 'm//s')
), checked AS (
    SELECT *, unit_convert_checked(value, source_unit, 'kW.h') AS result
    FROM readings
)
SELECT id, source_unit, round(result.value, 6) AS kwh, result.status,
       split_part(result.message, E'\n', 1) AS message
FROM checked ORDER BY id;

.print ''
.print '5. Inspect a unit: base_value = value * factor + offset'
SELECT u AS source_unit, p.dimension, p.base_unit, p.factor, p.offset, p.is_affine
FROM (SELECT u, unit_parse(u) AS p FROM (VALUES ('kg/m3'), ('kW.h'), ('Cel')) t(u));

.print ''
.print '6. Unit symbols are case-sensitive: MW is not mW'
SELECT unit_convert(1, 'MW', 'mW') AS milliwatts_in_one_megawatt;

.print ''
.print 'Backend and policy provenance'
SELECT units_backend();

.print ''
.print 'Strict conversion rejects incompatible dimensions. Try interactively:'
.print "SELECT unit_convert(1, 'kg', 'kW.h');"
