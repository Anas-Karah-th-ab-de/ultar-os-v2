//! The bus service `auth`: one request at a time (checks are deliberately slow).

use crate::service::Auth;
use std::time::Duration;
use ultar_bus::messages::auth::{
    SetSecret, SetSecretReply, Users, UsersReply, Verify, VerifyReply, ADMIN_METHOD, SERVICE,
};
use ultar_bus::messages::auth_admin::{ManageReply, RemoveUser, UpdateUser};
use ultar_bus::IncomingRequest;
use ultar_bus::{BusError, Call, ErrorCode};

/// Decides whether a caller is an administrator of `auth` (the policy's pseudo method).
pub trait AdminCheck: Send {
    /// True for administrators.
    fn is_admin(&self, caller: &str) -> bool;
}

/// Administrators by a fixed list of identities (tests, tools).
#[derive(Debug, Clone, Default)]
pub struct AdminList(pub Vec<String>);

impl AdminCheck for AdminList {
    fn is_admin(&self, caller: &str) -> bool {
        self.0.iter().any(|a| a == caller)
    }
}

/// Administrators by the bus policy: identities that may call the pseudo method
/// `auth.admin` (the same file the broker enforces).
impl AdminCheck for ultar_bus::Policy {
    fn is_admin(&self, caller: &str) -> bool {
        ultar_bus::Identity::new(caller).is_ok_and(|id| self.may_call(&id, SERVICE, ADMIN_METHOD))
    }
}

/// Answers one request.
pub fn handle(
    auth: &mut Auth,
    admins: &dyn AdminCheck,
    request: IncomingRequest,
) -> Result<(), BusError> {
    match request.method() {
        m if m == Users::METHOD => request.respond::<Users>(|_| {
            Ok(UsersReply {
                users: auth.users(),
            })
        }),
        m if m == Verify::METHOD => request.respond::<Verify>(|v| {
            Ok(VerifyReply {
                outcome: auth.verify(&v.user, v.method, &v.secret),
            })
        }),
        m if m == SetSecret::METHOD => {
            let admin = admins.is_admin(request.caller());
            request.respond::<SetSecret>(|s| {
                Ok(SetSecretReply {
                    outcome: auth.set_secret(&s, admin),
                })
            })
        }
        m if m == UpdateUser::METHOD => {
            let admin = admins.is_admin(request.caller());
            request.respond::<UpdateUser>(|u| {
                Ok(ManageReply {
                    outcome: auth.update_user(&u, admin),
                })
            })
        }
        m if m == RemoveUser::METHOD => {
            let admin = admins.is_admin(request.caller());
            request.respond::<RemoveUser>(|r| {
                Ok(ManageReply {
                    outcome: auth.remove_user(&r, admin),
                })
            })
        }
        _ => request.fail(ErrorCode::UnknownMethod),
    }
}

/// Serves until `stop` returns true (checked at least every `tick`).
pub fn serve(
    endpoint: &ultar_bus::ServiceEndpoint,
    auth: &mut Auth,
    admins: &dyn AdminCheck,
    tick: Duration,
    stop: &dyn Fn() -> bool,
) -> Result<(), BusError> {
    while !stop() {
        match endpoint.recv_timeout(tick) {
            Ok(request) => handle(auth, admins, request)?,
            Err(BusError::Timeout) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
