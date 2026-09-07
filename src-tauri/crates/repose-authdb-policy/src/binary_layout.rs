use crate::transform::PolicyError;

const HEADER: &[u8; 8] = b"bplist00";
const TRAILER_BYTES: usize = 32;
const MINIMUM_BINARY_BYTES: usize = HEADER.len() + 1 + 1 + TRAILER_BYTES;

#[derive(Clone, Copy, Debug)]
pub(crate) struct BinaryLimits {
    pub(crate) maximum_objects: usize,
    pub(crate) maximum_array_items: usize,
    pub(crate) maximum_dictionary_items: usize,
}

#[derive(Clone, Copy, Debug)]
struct References {
    start: usize,
    count: usize,
}

#[derive(Clone, Copy, Debug)]
struct ObjectMetadata {
    references: Option<References>,
}

pub(crate) fn validate_binary_layout(
    input: &[u8],
    limits: BinaryLimits,
) -> Result<(), PolicyError> {
    if input.len() < MINIMUM_BINARY_BYTES || input.get(..HEADER.len()) != Some(HEADER) {
        return Err(invalid("missing header or complete trailer"));
    }

    let trailer_start = input
        .len()
        .checked_sub(TRAILER_BYTES)
        .ok_or_else(|| invalid("missing trailer"))?;
    let trailer = input
        .get(trailer_start..)
        .ok_or_else(|| invalid("missing trailer"))?;
    if trailer.get(..6) != Some(&[0; 6]) {
        return Err(invalid("nonzero trailer reserved bytes"));
    }

    let offset_width = usize::from(trailer[6]);
    let reference_width = usize::from(trailer[7]);
    if !valid_width(offset_width) || !valid_width(reference_width) {
        return Err(invalid("invalid offset or object-reference width"));
    }

    let declared_objects_u64 = read_u64(trailer, 8)?;
    let root_object_u64 = read_u64(trailer, 16)?;
    let offset_table_start_u64 = read_u64(trailer, 24)?;
    let declared_objects = usize::try_from(declared_objects_u64)
        .map_err(|_| invalid("object count is not addressable"))?;
    if declared_objects == 0 {
        return Err(invalid("binary plist declares no objects"));
    }
    if declared_objects > limits.maximum_objects {
        return Err(PolicyError::BinaryObjectLimitExceeded {
            declared: declared_objects,
            maximum: limits.maximum_objects,
        });
    }
    let root_object = usize::try_from(root_object_u64)
        .map_err(|_| invalid("root object reference is not addressable"))?;
    if root_object >= declared_objects {
        return Err(invalid("root object reference is out of range"));
    }
    let offset_table_start = usize::try_from(offset_table_start_u64)
        .map_err(|_| invalid("offset table location is not addressable"))?;
    if offset_table_start < HEADER.len() || offset_table_start >= trailer_start {
        return Err(invalid("offset table is outside the object region"));
    }

    let offset_table_bytes = declared_objects
        .checked_mul(offset_width)
        .ok_or_else(|| invalid("offset table size overflow"))?;
    let offset_table_end = offset_table_start
        .checked_add(offset_table_bytes)
        .ok_or_else(|| invalid("offset table end overflow"))?;
    if offset_table_end != trailer_start {
        return Err(invalid("offset table is not adjacent to the final trailer"));
    }

    let mut physical_objects = Vec::with_capacity(declared_objects);
    for object_index in 0..declared_objects {
        let entry_offset = object_index
            .checked_mul(offset_width)
            .and_then(|relative| offset_table_start.checked_add(relative))
            .ok_or_else(|| invalid("offset-table entry overflow"))?;
        let object_offset = read_uint(input, entry_offset, offset_width, trailer_start)?;
        if object_offset < HEADER.len() || object_offset >= offset_table_start {
            return Err(invalid("object offset is outside the object region"));
        }
        physical_objects.push((object_offset, object_index));
    }
    physical_objects.sort_unstable_by_key(|(offset, _)| *offset);
    if physical_objects
        .windows(2)
        .any(|pair| pair[0].0 == pair[1].0)
    {
        return Err(invalid("duplicate object offsets"));
    }

    let mut metadata: Vec<Option<ObjectMetadata>> = vec![None; declared_objects];
    let mut expected_offset = HEADER.len();
    for (object_offset, object_index) in physical_objects {
        if object_offset != expected_offset {
            return Err(invalid("object region contains a gap or overlap"));
        }
        let (end, object_metadata) = parse_object(
            input,
            object_offset,
            offset_table_start,
            reference_width,
            limits,
        )?;
        expected_offset = end;
        metadata[object_index] = Some(object_metadata);
    }
    if expected_offset != offset_table_start {
        return Err(invalid("object region does not end at the offset table"));
    }
    let metadata: Vec<ObjectMetadata> = metadata
        .into_iter()
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| invalid("offset table does not describe every object"))?;

    validate_references(input, &metadata, reference_width)?;
    validate_reachable_acyclic_graph(input, &metadata, root_object, reference_width)
}

fn parse_object(
    input: &[u8],
    start: usize,
    object_region_end: usize,
    reference_width: usize,
    limits: BinaryLimits,
) -> Result<(usize, ObjectMetadata), PolicyError> {
    let token = *input
        .get(start)
        .ok_or_else(|| invalid("object token is out of bounds"))?;
    let kind = token & 0xf0;
    let size = token & 0x0f;

    let (end, references) = match (kind, size) {
        (0x00, 0x08 | 0x09) => (checked_end(start, 1, object_region_end)?, None),
        (0x10, exponent @ 0..=4) => {
            let payload = 1usize
                .checked_shl(u32::from(exponent))
                .ok_or_else(|| invalid("integer width overflow"))?;
            let total = payload
                .checked_add(1)
                .ok_or_else(|| invalid("integer extent overflow"))?;
            (checked_end(start, total, object_region_end)?, None)
        }
        (0x20, 2) => (checked_end(start, 5, object_region_end)?, None),
        (0x20, 3) => (checked_end(start, 9, object_region_end)?, None),
        (0x30, 3) => (checked_end(start, 9, object_region_end)?, None),
        (0x40 | 0x50, encoded_length) => {
            let (length, payload_start) =
                parse_length(input, start, encoded_length, object_region_end)?;
            (checked_end(payload_start, length, object_region_end)?, None)
        }
        (0x60, encoded_length) => {
            let (code_units, payload_start) =
                parse_length(input, start, encoded_length, object_region_end)?;
            let payload_bytes = code_units
                .checked_mul(2)
                .ok_or_else(|| invalid("UTF-16 payload size overflow"))?;
            (
                checked_end(payload_start, payload_bytes, object_region_end)?,
                None,
            )
        }
        (0x80, payload_minus_one @ 0..=7) => {
            let payload = usize::from(payload_minus_one)
                .checked_add(1)
                .ok_or_else(|| invalid("UID width overflow"))?;
            let total = payload
                .checked_add(1)
                .ok_or_else(|| invalid("UID extent overflow"))?;
            (checked_end(start, total, object_region_end)?, None)
        }
        (0xa0, encoded_count) => {
            let (count, references_start) =
                parse_length(input, start, encoded_count, object_region_end)?;
            if count > limits.maximum_array_items {
                return Err(PolicyError::BinaryCollectionLimitExceeded {
                    kind: "array",
                    declared: count,
                    maximum: limits.maximum_array_items,
                });
            }
            let reference_bytes = count
                .checked_mul(reference_width)
                .ok_or_else(|| invalid("array reference size overflow"))?;
            (
                checked_end(references_start, reference_bytes, object_region_end)?,
                Some(References {
                    start: references_start,
                    count,
                }),
            )
        }
        (0xd0, encoded_count) => {
            let (pairs, references_start) =
                parse_length(input, start, encoded_count, object_region_end)?;
            if pairs > limits.maximum_dictionary_items {
                return Err(PolicyError::BinaryCollectionLimitExceeded {
                    kind: "dictionary",
                    declared: pairs,
                    maximum: limits.maximum_dictionary_items,
                });
            }
            let reference_count = pairs
                .checked_mul(2)
                .ok_or_else(|| invalid("dictionary reference count overflow"))?;
            let reference_bytes = reference_count
                .checked_mul(reference_width)
                .ok_or_else(|| invalid("dictionary reference size overflow"))?;
            (
                checked_end(references_start, reference_bytes, object_region_end)?,
                Some(References {
                    start: references_start,
                    count: reference_count,
                }),
            )
        }
        _ => return Err(invalid("unsupported or ambiguous object token")),
    };

    Ok((end, ObjectMetadata { references }))
}

fn parse_length(
    input: &[u8],
    object_start: usize,
    encoded_length: u8,
    object_region_end: usize,
) -> Result<(usize, usize), PolicyError> {
    if encoded_length < 0x0f {
        let payload_start = object_start
            .checked_add(1)
            .ok_or_else(|| invalid("inline-length prefix overflow"))?;
        return Ok((usize::from(encoded_length), payload_start));
    }

    let marker_offset = object_start
        .checked_add(1)
        .ok_or_else(|| invalid("length-marker offset overflow"))?;
    let marker = *input
        .get(marker_offset)
        .ok_or_else(|| invalid("missing extended-length marker"))?;
    let length_width = match marker {
        0x10 => 1,
        0x11 => 2,
        0x12 => 4,
        0x13 => 8,
        _ => return Err(invalid("invalid extended-length integer marker")),
    };
    let length_offset = marker_offset
        .checked_add(1)
        .ok_or_else(|| invalid("extended-length offset overflow"))?;
    let length = read_uint(input, length_offset, length_width, object_region_end)?;
    let payload_start = length_offset
        .checked_add(length_width)
        .ok_or_else(|| invalid("extended-length prefix overflow"))?;
    Ok((length, payload_start))
}

fn validate_references(
    input: &[u8],
    metadata: &[ObjectMetadata],
    reference_width: usize,
) -> Result<(), PolicyError> {
    for object in metadata {
        if let Some(references) = object.references {
            for reference_index in 0..references.count {
                let reference =
                    read_reference(input, references, reference_index, reference_width)?;
                if reference >= metadata.len() {
                    return Err(invalid("collection reference is out of range"));
                }
            }
        }
    }
    Ok(())
}

fn validate_reachable_acyclic_graph(
    input: &[u8],
    metadata: &[ObjectMetadata],
    root_object: usize,
    reference_width: usize,
) -> Result<(), PolicyError> {
    let mut colors = vec![0u8; metadata.len()];
    colors[root_object] = 1;
    let mut stack = vec![(root_object, 0usize)];

    while let Some(&(object_index, next_reference)) = stack.last() {
        let reference_count = metadata[object_index]
            .references
            .map_or(0, |references| references.count);
        if next_reference == reference_count {
            colors[object_index] = 2;
            stack.pop();
            continue;
        }

        let next = next_reference
            .checked_add(1)
            .ok_or_else(|| invalid("graph reference index overflow"))?;
        stack
            .last_mut()
            .ok_or_else(|| invalid("graph traversal stack was lost"))?
            .1 = next;
        let references = metadata[object_index]
            .references
            .ok_or_else(|| invalid("scalar object unexpectedly has references"))?;
        let child = read_reference(input, references, next_reference, reference_width)?;
        match colors[child] {
            0 => {
                colors[child] = 1;
                stack.push((child, 0));
            }
            1 => return Err(invalid("binary object graph contains a cycle")),
            2 => {}
            _ => return Err(invalid("invalid graph traversal state")),
        }
    }

    if colors.into_iter().any(|color| color == 0) {
        return Err(invalid("binary object graph contains unreachable objects"));
    }
    Ok(())
}

fn read_reference(
    input: &[u8],
    references: References,
    reference_index: usize,
    reference_width: usize,
) -> Result<usize, PolicyError> {
    if reference_index >= references.count {
        return Err(invalid("reference index is out of range"));
    }
    let offset = reference_index
        .checked_mul(reference_width)
        .and_then(|relative| references.start.checked_add(relative))
        .ok_or_else(|| invalid("reference offset overflow"))?;
    read_uint(input, offset, reference_width, input.len())
}

fn checked_end(start: usize, length: usize, limit: usize) -> Result<usize, PolicyError> {
    let end = start
        .checked_add(length)
        .ok_or_else(|| invalid("object extent overflow"))?;
    if end > limit {
        Err(invalid("object extent exceeds the object region"))
    } else {
        Ok(end)
    }
}

fn read_u64(input: &[u8], start: usize) -> Result<u64, PolicyError> {
    let end = start
        .checked_add(8)
        .ok_or_else(|| invalid("integer offset overflow"))?;
    let bytes: [u8; 8] = input
        .get(start..end)
        .ok_or_else(|| invalid("truncated trailer integer"))?
        .try_into()
        .map_err(|_| invalid("invalid trailer integer width"))?;
    Ok(u64::from_be_bytes(bytes))
}

fn read_uint(input: &[u8], start: usize, width: usize, limit: usize) -> Result<usize, PolicyError> {
    let end = start
        .checked_add(width)
        .ok_or_else(|| invalid("unsigned integer extent overflow"))?;
    if end > limit {
        return Err(invalid("truncated unsigned integer"));
    }
    let bytes = input
        .get(start..end)
        .ok_or_else(|| invalid("truncated unsigned integer"))?;
    let mut value = 0u64;
    for byte in bytes {
        value = value
            .checked_shl(8)
            .map(|value| value | u64::from(*byte))
            .ok_or_else(|| invalid("unsigned integer overflow"))?;
    }
    usize::try_from(value).map_err(|_| invalid("unsigned integer is not addressable"))
}

fn valid_width(width: usize) -> bool {
    matches!(width, 1 | 2 | 3 | 4 | 8)
}

fn invalid(reason: &'static str) -> PolicyError {
    PolicyError::InvalidBinaryLayout { reason }
}
