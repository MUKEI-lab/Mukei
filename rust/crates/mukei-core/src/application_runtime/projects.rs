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
        let Some(existing) = self.features.project(&payload.project_id) else {
            return CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::StaleScope,
            );
        };
        if existing.status == ProjectStatus::Archived {
            return CommandAcknowledgementV2::rejected(
                Some(&command.envelope),
                RejectionReason::PolicyDenied,
            );
        }

        let (acknowledgement, operation_id, _) = self.accept_operation(command);
        let name = payload.name.trim().to_owned();
        let description = payload.description.trim().to_owned();
        let updated = self
            .features
            .update_project(&payload.project_id, |project| {
                project.name = name;
                project.description = description;
                project.updated_at = Utc::now();
            })
            .expect("project existence checked before update");
        self.complete_project_operation(command, &operation_id, "project.updated", &updated);
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
        let result = serde_json::to_value(project).unwrap_or(Value::Null);
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
