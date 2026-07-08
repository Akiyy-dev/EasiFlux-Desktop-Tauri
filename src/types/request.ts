export type RequestStatus = 'idle' | 'loading' | 'success' | 'error' | 'empty'

export interface AsyncState<T> {
  status: RequestStatus
  data: T | null
  error: string | null
  updatedAt: number | null
}
