const MAX_PROJECT_MEMORY_ENTRIES: usize = 16;

fn project_memory_index(project: &ProjectProjection, memory_id: &str) -> Option<usize> {
    project
        .memory
        .iter()
        .position(|entry| entry.memory_id == memory_id)
}

impl FeatureState {
    fn persist_projects(&self) {
        let _enqueue = self
            .persistence_enqueue
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut records = self
            .projects
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .values()
            .cloned()
            .collect::<Vec<_>>();
        records.sort_by(|left, right| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then_with(|| left.project_id.cmp(&right.project_id))
        });
        for project in &mut records {
            project.memory.sort_by(|left, right| {
                right
                    .updated_at
                    .cmp(&left.updated_at)
                    .then_with(|| left.memory_id.cmp(&right.memory_id))
            });
        }
        if let Ok(value) = serde_json::to_value(records) {
            self.persist_value("projects", value);
        }
    }

    fn insert_project(&self, project: ProjectProjection) {
        self.projects
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(project.project_id.clone(), project);
        self.persist_projects();
    }

    fn project(&self, project_id: &str) -> Option<ProjectProjection> {
        self.projects
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(project_id)
            .cloned()
    }

    fn update_project(
        &self,
        project_id: &str,
        update: impl FnOnce(&mut ProjectProjection),
    ) -> Option<ProjectProjection> {
        let updated = {
            let mut projects = self
                .projects
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let project = projects.get_mut(project_id)?;
            update(project);
            project.clone()
        };
        self.persist_projects();
        Some(updated)
    }

    fn update_active_project(
    &self,
    project_id: &str,
    update: impl FnOnce(&mut ProjectProjection) -> Result<(), RejectionReason>,
) -> Result<ProjectProjection, RejectionReason> {
    let updated = {
        let mut projects = self
            .projects
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let project = projects
            .get_mut(project_id)
            .ok_or(RejectionReason::StaleScope)?;
        if project.status == ProjectStatus::Archived {
            return Err(RejectionReason::PolicyDenied);
        }
        update(project)?;
        project.clone()
    };
    self.persist_projects();
    Ok(updated)
}

    fn projects_snapshot(&self) -> Value {
        let mut projects = self
            .projects
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .values()
            .cloned()
            .collect::<Vec<_>>();
        projects.sort_by(|left, right| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then_with(|| left.project_id.cmp(&right.project_id))
        });
        for project in &mut projects {
            project.memory.sort_by(|left, right| {
                right
                    .updated_at
                    .cmp(&left.updated_at)
                    .then_with(|| left.memory_id.cmp(&right.memory_id))
            });
        }
        json!({ "projects": projects })
    }
}

impl MukeiRuntime {


    fn create_project(&self, command: &ValidatedCommand) -> CommandAcknowledgementV2 {
        if let Err(acknowledgement) = self.ensure_ready(command) {
            return acknowledgement;
        }
        let ValidatedCommandPayload::ProjectCreate(payload) = &command.payload else {
            return CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::InvalidPayload,
            );
        };

        let (acknowledgement, operation_id, _) = self.accept_operation(command);
        let now = Utc::now();
        let project = ProjectProjection {
            project_id: Uuid::new_v4().to_string(),
            name: payload.name.trim().to_owned(),
            description: payload.description.trim().to_owned(),
            instructions: String::new(),
            memory: Vec::new(),
            status: ProjectStatus::Active,
            created_at: now,
            updated_at: now,
        };
        self.features.insert_project(project.clone());
        self.complete_project_operation(command, &operation_id, "project.created", &project);
        acknowledgement
    }

    fn update_project(&self, command: &ValidatedCommand) -> CommandAcknowledgementV2 {
    if let Err(acknowledgement) = self.ensure_ready(command) {
        return acknowledgement;
    }
    let ValidatedCommandPayload::ProjectUpdate(payload) = &command.payload else {
        return CommandAcknowledgementV2::rejected(
            Some(&command.envelope),
            RejectionReason::InvalidPayload,
        );
    };
    let name = payload.name.trim().to_owned();
    let description = payload.description.trim().to_owned();
    let updated = match self.features.update_active_project(&payload.project_id, |project| {
        project.name = name;
        project.description = description;
        project.updated_at = Utc::now();
        Ok(())
    }) {
        Ok(project) => project,
        Err(reason) => {
            return CommandAcknowledgementV2::rejected(Some(&command.envelope), reason)
        }
    };
    let (acknowledgement, operation_id, _) = self.accept_operation(command);
    self.complete_project_operation(command, &operation_id, "project.updated", &updated);
    acknowledgement
}

    fn update_project_instructions(&self, command: &ValidatedCommand) -> CommandAcknowledgementV2 {
    if let Err(acknowledgement) = self.ensure_ready(command) {
        return acknowledgement;
    }
    let ValidatedCommandPayload::ProjectInstructions(payload) = &command.payload else {
        return CommandAcknowledgementV2::rejected(
            Some(&command.envelope),
            RejectionReason::InvalidPayload,
        );
    };
    let instructions = payload.instructions.trim().to_owned();
    let updated = match self.features.update_active_project(&payload.project_id, |project| {
        project.instructions = instructions;
        project.updated_at = Utc::now();
        Ok(())
    }) {
        Ok(project) => project,
        Err(reason) => {
            return CommandAcknowledgementV2::rejected(Some(&command.envelope), reason)
        }
    };
    let (acknowledgement, operation_id, _) = self.accept_operation(command);
    self.complete_project_context_operation(
        command,
        &operation_id,
        "project.instructions.updated",
        json!({"project_id": updated.project_id, "updated_at": updated.updated_at}),
    );
    acknowledgement
}

    fn add_project_memory(&self, command: &ValidatedCommand) -> CommandAcknowledgementV2 {
    if let Err(acknowledgement) = self.ensure_ready(command) {
        return acknowledgement;
    }
    let ValidatedCommandPayload::ProjectMemoryCreate(payload) = &command.payload else {
        return CommandAcknowledgementV2::rejected(
            Some(&command.envelope),
            RejectionReason::InvalidPayload,
        );
    };
    let now = Utc::now();
    let memory_id = Uuid::new_v4().to_string();
    let content = payload.content.trim().to_owned();
    let updated = match self.features.update_active_project(&payload.project_id, |project| {
        if project.memory.len() >= MAX_PROJECT_MEMORY_ENTRIES {
            return Err(RejectionReason::PolicyDenied);
        }
        project.memory.push(ProjectMemoryEntry {
            memory_id: memory_id.clone(),
            content,
            created_at: now,
            updated_at: now,
        });
        project.updated_at = now;
        Ok(())
    }) {
        Ok(project) => project,
        Err(reason) => {
            return CommandAcknowledgementV2::rejected(Some(&command.envelope), reason)
        }
    };
    let (acknowledgement, operation_id, _) = self.accept_operation(command);
    self.complete_project_context_operation(
        command,
        &operation_id,
        "project.memory.added",
        json!({
            "project_id": updated.project_id,
            "memory_id": memory_id,
            "updated_at": updated.updated_at,
        }),
    );
    acknowledgement
}

    fn update_project_memory(&self, command: &ValidatedCommand) -> CommandAcknowledgementV2 {
    if let Err(acknowledgement) = self.ensure_ready(command) {
        return acknowledgement;
    }
    let ValidatedCommandPayload::ProjectMemoryUpdate(payload) = &command.payload else {
        return CommandAcknowledgementV2::rejected(
            Some(&command.envelope),
            RejectionReason::InvalidPayload,
        );
    };
    let now = Utc::now();
    let content = payload.content.trim().to_owned();
    let memory_id = payload.memory_id.clone();
    let updated = match self.features.update_active_project(&payload.project_id, |project| {
        let memory_index = project_memory_index(project, &memory_id)
            .ok_or(RejectionReason::StaleScope)?;
        let entry = &mut project.memory[memory_index];
        entry.content = content;
        entry.updated_at = now;
        project.updated_at = now;
        Ok(())
    }) {
        Ok(project) => project,
        Err(reason) => {
            return CommandAcknowledgementV2::rejected(Some(&command.envelope), reason)
        }
    };
    let (acknowledgement, operation_id, _) = self.accept_operation(command);
    self.complete_project_context_operation(
        command,
        &operation_id,
        "project.memory.updated",
        json!({
            "project_id": updated.project_id,
            "memory_id": memory_id,
            "updated_at": updated.updated_at,
        }),
    );
    acknowledgement
}

    fn delete_project_memory(&self, command: &ValidatedCommand) -> CommandAcknowledgementV2 {
    if let Err(acknowledgement) = self.ensure_ready(command) {
        return acknowledgement;
    }
    let ValidatedCommandPayload::ProjectMemory(payload) = &command.payload else {
        return CommandAcknowledgementV2::rejected(
            Some(&command.envelope),
            RejectionReason::InvalidPayload,
        );
    };
    let now = Utc::now();
    let memory_id = payload.memory_id.clone();
    let updated = match self.features.update_active_project(&payload.project_id, |project| {
        let memory_index = project_memory_index(project, &memory_id)
            .ok_or(RejectionReason::StaleScope)?;
        project.memory.remove(memory_index);
        project.updated_at = now;
        Ok(())
    }) {
        Ok(project) => project,
        Err(reason) => {
            return CommandAcknowledgementV2::rejected(Some(&command.envelope), reason)
        }
    };
    let (acknowledgement, operation_id, _) = self.accept_operation(command);
    self.complete_project_context_operation(
        command,
        &operation_id,
        "project.memory.deleted",
        json!({
            "project_id": updated.project_id,
            "memory_id": memory_id,
            "updated_at": updated.updated_at,
        }),
    );
    acknowledgement
}

    fn archive_project(&self, command: &ValidatedCommand) -> CommandAcknowledgementV2 {
        if let Err(acknowledgement) = self.ensure_ready(command) {
            return acknowledgement;
        }
        let ValidatedCommandPayload::Project(payload) = &command.payload else {
            return CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::InvalidPayload,
            );
        };
        if self.features.project(&payload.project_id).is_none() {
            return CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::StaleScope,
            );
        }

        let (acknowledgement, operation_id, _) = self.accept_operation(command);
        let updated = self
            .features
            .update_project(&payload.project_id, |project| {
                project.status = ProjectStatus::Archived;
                project.updated_at = Utc::now();
            })
            .expect("project existence checked before archive");
        self.complete_project_operation(command, &operation_id, "project.archived", &updated);
        acknowledgement
    }

    fn complete_project_operation(
    &self,
    command: &ValidatedCommand,
    operation_id: &str,
    event_type: &str,
    project: &ProjectProjection,
) {
    let result = json!({
        "project_id": project.project_id,
        "status": project.status,
        "created_at": project.created_at,
        "updated_at": project.updated_at,
    });
    self.features.update_operation(
        operation_id,
        OperationStatus::Completed,
        Some(1.0),
        None,
        result.clone(),
    );
    self.events.emit(
        "application:projects",
        event_type,
        result,
        Some(&command.envelope),
        Some(operation_id.to_owned()),
    );
    self.events.emit(
        &format!("operation:{operation_id}"),
        "operation.completed",
        json!({"state": "completed"}),
        Some(&command.envelope),
        Some(operation_id.to_owned()),
    );
}

    fn complete_project_context_operation(
        &self,
        command: &ValidatedCommand,
        operation_id: &str,
        event_type: &str,
        result: Value,
    ) {
        self.features.update_operation(
            operation_id,
            OperationStatus::Completed,
            Some(1.0),
            None,
            result.clone(),
        );
        self.events.emit(
            "application:projects",
            event_type,
            result,
            Some(&command.envelope),
            Some(operation_id.to_owned()),
        );
        self.events.emit(
            &format!("operation:{operation_id}"),
            "operation.completed",
            json!({"state": "completed"}),
            Some(&command.envelope),
            Some(operation_id.to_owned()),
        );
    }
}

#[cfg(test)]
mod project_context_tests {
    use super::*;

    #[test]
    fn legacy_project_projection_defaults_new_context_fields() {
        let value = json!({
            "project_id": "project-1",
            "name": "Legacy",
            "description": "existing record",
            "status": "active",
            "created_at": "2026-07-19T00:00:00Z",
            "updated_at": "2026-07-19T00:00:00Z"
        });
        let project: ProjectProjection = serde_json::from_value(value).expect("legacy project");
        assert!(project.instructions.is_empty());
        assert!(project.memory.is_empty());
    }

    #[test]
    fn project_operation_summary_shape_excludes_private_context() {
        let now = Utc::now();
        let project = ProjectProjection {
            project_id: "project-private".into(),
            name: "name-secret".into(),
            description: "description-secret".into(),
            instructions: "instructions-secret".into(),
            memory: vec![ProjectMemoryEntry {
                memory_id: "memory-private".into(),
                content: "memory-secret".into(),
                created_at: now,
                updated_at: now,
            }],
            status: ProjectStatus::Active,
            created_at: now,
            updated_at: now,
        };
        let summary = json!({
            "project_id": project.project_id,
            "status": project.status,
            "created_at": project.created_at,
            "updated_at": project.updated_at,
        })
        .to_string();
        assert!(!summary.contains("name-secret"));
        assert!(!summary.contains("description-secret"));
        assert!(!summary.contains("instructions-secret"));
        assert!(!summary.contains("memory-secret"));
    }

    #[test]
    fn memory_identity_lookup_never_crosses_project_boundary() {
        let now = Utc::now();
        let memory = ProjectMemoryEntry {
            memory_id: "memory-a".into(),
            content: "Only project A".into(),
            created_at: now,
            updated_at: now,
        };
        let project_a = ProjectProjection {
            project_id: "project-a".into(),
            name: "A".into(),
            description: String::new(),
            instructions: String::new(),
            memory: vec![memory],
            status: ProjectStatus::Active,
            created_at: now,
            updated_at: now,
        };
        let project_b = ProjectProjection {
            project_id: "project-b".into(),
            name: "B".into(),
            description: String::new(),
            instructions: String::new(),
            memory: Vec::new(),
            status: ProjectStatus::Active,
            created_at: now,
            updated_at: now,
        };
        assert_eq!(project_memory_index(&project_a, "memory-a"), Some(0));
        assert_eq!(project_memory_index(&project_b, "memory-a"), None);
    }
}
