export interface UserRead {
  id: number;
  email: string;
  full_name: string;
  createdAt: string;
  is_active: boolean;
}

export async function getUser(id: number): Promise<UserRead> {
  const res = await fetch(`/users/${id}`);
  if (!res.ok) throw new Error(`GET /users/${id} failed: ${res.status}`);
  return (await res.json()) as UserRead;
}

export function displayName(user: UserRead): string {
  return user.full_name;
}
